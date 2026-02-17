// End-to-end pipeline max throughput benchmarks
//
// Replicates compiled PDL pipeline patterns without timer pacing to measure
// the maximum achievable throughput.
//
// Equivalent PDL (pipeline only):
//   clock <max> pipeline { constant(1.0) | mul(2.0) | mul(0.5) }
//
// Equivalent PDL (socket loopback):
//   clock <max> sender   { constant(1.0) | mul(2.0) | socket_write("localhost:19876", 0) }
//   clock <max> receiver { socket_read("localhost:19876") | mul(0.5) }

#include <benchmark/benchmark.h>

#include <atomic>
#include <chrono>
#include <cstring>
#include <pipit.h>
#include <pipit_net.h>
#include <std_actors.h>
#include <thread>
#include <vector>

using Clock = std::chrono::steady_clock;

// ── Pipeline only: max compute throughput ───────────────────────────────────
//
// Fires the actor chain in a tight loop with no timer overhead.
// Measures the CPU-bound throughput ceiling.

static void BM_E2E_PipelineOnly(benchmark::State &state) {
    const int N = static_cast<int>(state.range(0));

    // Edge buffers (same as codegen allocates between actors)
    std::vector<float> edge0(N);
    std::vector<float> edge1(N);
    std::vector<float> edge2(N);

    // Actor instances (same structs as codegen creates)
    Actor_constant gen{1.0f, N};
    Actor_mul mul1{2.0f, N};
    Actor_mul mul2{0.5f, N};

    for (auto _ : state) {
        // PASS firing order: constant → mul(2.0) → mul(0.5)
        gen(nullptr, edge0.data());
        mul1(edge0.data(), edge1.data());
        mul2(edge1.data(), edge2.data());
        benchmark::DoNotOptimize(edge2.data());
    }

    state.SetItemsProcessed(state.iterations() * N);
    state.SetBytesProcessed(state.iterations() * N * static_cast<int64_t>(sizeof(float)));
}

BENCHMARK(BM_E2E_PipelineOnly)->Arg(1)->Arg(64)->Arg(256)->Arg(1024)->Unit(benchmark::kNanosecond);

// ── Pipeline + socket loopback: network-bound throughput ────────────────────
//
// Sender thread:  constant → mul(2.0) → PPKT send (UDP localhost)
// Receiver thread: recv → mul(0.5) → discard
//
// Uses raw sockets so we can enlarge SO_RCVBUF.  Sender is non-blocking
// (same as pipit runtime — silent drop on EAGAIN).  Packets are chunked at
// MTU boundaries via ppkt_send_chunked, matching real runtime behavior.
//
// Runs for a fixed duration (2s) to reach steady-state throughput.
// The key metric is rx_samples_per_sec — the receiver's sustained drain rate.

static constexpr uint16_t BENCH_PORT = 19876;
static constexpr int BENCH_DURATION_MS = 2000;

/// Open a non-blocking UDP socket bound to localhost:port for receiving.
/// Enlarges SO_RCVBUF to the kernel maximum.
static int open_rx_socket(uint16_t port) {
    int fd = ::socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0)
        return -1;

    int optval = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &optval, sizeof(optval));

    // Enlarge receive buffer (kernel caps at 2 × rmem_max)
    int bufsize = 16 * 1024 * 1024;
    setsockopt(fd, SOL_SOCKET, SO_RCVBUF, &bufsize, sizeof(bufsize));

    struct sockaddr_in addr{};
    addr.sin_family = AF_INET;
    addr.sin_port = htons(port);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);

    if (::bind(fd, reinterpret_cast<struct sockaddr *>(&addr), sizeof(addr)) < 0) {
        ::close(fd);
        return -1;
    }

    int flags = fcntl(fd, F_GETFL, 0);
    fcntl(fd, F_SETFL, flags | O_NONBLOCK);
    return fd;
}

/// Open a non-blocking UDP socket for sending to localhost:port.
static int open_tx_socket(uint16_t port, struct sockaddr_in &dest) {
    int fd = ::socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0)
        return -1;

    int flags = fcntl(fd, F_GETFL, 0);
    fcntl(fd, F_SETFL, flags | O_NONBLOCK);

    dest = {};
    dest.sin_family = AF_INET;
    dest.sin_port = htons(port);
    dest.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    return fd;
}

/// Send N float samples as MTU-chunked PPKT packets (same as runtime).
/// Non-blocking: silently drops on EAGAIN, matching pipit socket_write.
static int ppkt_send_raw(int fd, const struct sockaddr_in &dest, pipit::net::PpktHeader &hdr,
                         const float *data, uint32_t n) {
    constexpr size_t MTU = pipit::net::PPKT_DEFAULT_MTU;
    constexpr size_t MAX_PAYLOAD = MTU - sizeof(pipit::net::PpktHeader);
    constexpr uint32_t MAX_SAMPLES = static_cast<uint32_t>(MAX_PAYLOAD / sizeof(float));

    alignas(8) uint8_t pkt[MTU];
    int sent = 0;
    uint32_t offset = 0;

    while (offset < n) {
        uint32_t chunk = (n - offset < MAX_SAMPLES) ? (n - offset) : MAX_SAMPLES;
        hdr.sample_count = chunk;
        hdr.payload_bytes = chunk * static_cast<uint32_t>(sizeof(float));

        size_t pkt_size = sizeof(pipit::net::PpktHeader) + hdr.payload_bytes;
        std::memcpy(pkt, &hdr, sizeof(hdr));
        std::memcpy(pkt + sizeof(hdr), data + offset, hdr.payload_bytes);

        ssize_t r = ::sendto(fd, pkt, pkt_size, 0, reinterpret_cast<const struct sockaddr *>(&dest),
                             sizeof(dest));
        if (r >= 0)
            ++sent;

        hdr.sequence++;
        offset += chunk;
    }
    return sent;
}

static void BM_E2E_SocketLoopback(benchmark::State &state) {
    const int N = static_cast<int>(state.range(0));

    for ([[maybe_unused]] auto _ : state) {
        int rx_fd = open_rx_socket(BENCH_PORT);
        if (rx_fd < 0) {
            state.SkipWithError("Failed to bind receiver on localhost:19876");
            return;
        }

        struct sockaddr_in tx_dest{};
        int tx_fd = open_tx_socket(BENCH_PORT, tx_dest);
        if (tx_fd < 0) {
            ::close(rx_fd);
            state.SkipWithError("Failed to open sender socket");
            return;
        }

        std::atomic<bool> stop{false};
        std::atomic<uint64_t> sent_samples{0};
        std::atomic<uint64_t> received_samples{0};

        // Receiver thread: tight recv loop → mul(0.5) → discard
        std::thread rx_thread([&] {
            uint8_t pkt[2048]; // MTU-sized packets only
            std::vector<float> rx_buf(N);

            auto drain_packet = [&](ssize_t n) {
                if (n <= static_cast<ssize_t>(sizeof(pipit::net::PpktHeader)))
                    return;
                pipit::net::PpktHeader hdr;
                std::memcpy(&hdr, pkt, sizeof(hdr));
                if (!pipit::net::ppkt_validate(hdr))
                    return;

                size_t payload_bytes = static_cast<size_t>(n) - sizeof(pipit::net::PpktHeader);
                size_t samples = payload_bytes / sizeof(float);
                const float *data =
                    reinterpret_cast<const float *>(pkt + sizeof(pipit::net::PpktHeader));
                size_t count =
                    (samples < static_cast<size_t>(N)) ? samples : static_cast<size_t>(N);

                // Pipeline: mul(0.5)
                for (size_t i = 0; i < count; ++i) {
                    rx_buf[i] = data[i] * 0.5f;
                }
                benchmark::DoNotOptimize(rx_buf.data());
                received_samples.fetch_add(count, std::memory_order_relaxed);
            };

            while (!stop.load(std::memory_order_acquire)) {
                ssize_t n = ::recvfrom(rx_fd, pkt, sizeof(pkt), 0, nullptr, nullptr);
                if (n > 0)
                    drain_packet(n);
            }

            // Drain remaining packets
            for (;;) {
                ssize_t n = ::recvfrom(rx_fd, pkt, sizeof(pkt), 0, nullptr, nullptr);
                if (n <= 0)
                    break;
                drain_packet(n);
            }
        });

        // Sender thread: blast for BENCH_DURATION_MS
        std::thread tx_thread([&] {
            std::vector<float> tx_buf0(N);
            std::vector<float> tx_buf1(N);
            Actor_constant gen{1.0f, N};
            Actor_mul mul1{2.0f, N};

            pipit::net::PpktHeader hdr = pipit::net::ppkt_make_header(pipit::net::DTYPE_F32, 0);

            while (!stop.load(std::memory_order_relaxed)) {
                gen(nullptr, tx_buf0.data());
                mul1(tx_buf0.data(), tx_buf1.data());
                ppkt_send_raw(tx_fd, tx_dest, hdr, tx_buf1.data(), static_cast<uint32_t>(N));
                sent_samples.fetch_add(N, std::memory_order_relaxed);
            }
        });

        // Run for fixed duration
        auto t0 = Clock::now();
        std::this_thread::sleep_for(std::chrono::milliseconds(BENCH_DURATION_MS));
        stop.store(true, std::memory_order_release);

        tx_thread.join();

        // Brief drain window
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
        // stop is already true, rx_thread will exit after drain
        rx_thread.join();
        auto t1 = Clock::now();

        ::close(tx_fd);
        ::close(rx_fd);

        double elapsed_s = std::chrono::duration<double>(t1 - t0).count();
        uint64_t tx_count = sent_samples.load(std::memory_order_relaxed);
        uint64_t rx_count = received_samples.load(std::memory_order_relaxed);

        state.SetIterationTime(elapsed_s);
        state.SetItemsProcessed(static_cast<int64_t>(rx_count));
        state.SetBytesProcessed(static_cast<int64_t>(rx_count) *
                                static_cast<int64_t>(sizeof(float)));

        state.counters["chunk_size"] = static_cast<double>(N);
        state.counters["sent_samples"] = static_cast<double>(tx_count);
        state.counters["received_samples"] = static_cast<double>(rx_count);
        state.counters["loss_pct"] =
            (tx_count > 0)
                ? 100.0 * (1.0 - static_cast<double>(rx_count) / static_cast<double>(tx_count))
                : 0.0;
        state.counters["rx_samples_per_sec"] =
            benchmark::Counter(static_cast<double>(rx_count), benchmark::Counter::kIsRate);
        state.counters["tx_samples_per_sec"] =
            benchmark::Counter(static_cast<double>(tx_count), benchmark::Counter::kIsRate);
    }
}

BENCHMARK(BM_E2E_SocketLoopback)
    ->Arg(64)
    ->Arg(256)
    ->Arg(1024)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

BENCHMARK_MAIN();
