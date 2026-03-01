// SHM shared-memory throughput benchmarks
//
// Measures PSHM ring throughput in the same pattern as BM_E2E_SocketLoopback
// so the two transports can be compared directly.
//
// Writer thread:  constant → mul(2.0) → SHM publish
// Reader thread:  SHM consume → mul(0.5) → discard
//
// Uses ShmWriter/ShmReader directly (not ShmIoAdapter) to isolate protocol
// overhead from the adapter's lazy-init and BindState machinery.

#include <benchmark/benchmark.h>

#include <atomic>
#include <chrono>
#include <cstring>
#include <pipit.h>
#include <pipit_net.h>
#include <pipit_shm.h>
#include <std_actors.h>
#include <thread>
#include <vector>

using Clock = std::chrono::steady_clock;

static constexpr int SHM_BENCH_DURATION_MS = 2000;
static constexpr uint32_t SHM_BENCH_SLOTS = 256;

// ── SHM loopback: shared-memory throughput ──────────────────────────────────
//
// Writer thread: constant(1.0) → mul(2.0) → ShmWriter::publish()
// Reader thread: ShmReader::consume() → mul(0.5) → discard
//
// Fixed 2s duration, 1 iteration — matches BM_E2E_SocketLoopback format.
// The key metric is rx_samples_per_sec for direct comparison with UDP.

static void BM_SHM_Loopback(benchmark::State &state) {
    const int N = static_cast<int>(state.range(0));
    const uint32_t payload_bytes = static_cast<uint32_t>(N * sizeof(float));

    // slot_bytes must be >= payload and multiple of 8
    uint32_t slot_bytes = (payload_bytes + 7u) & ~7u;
    if (slot_bytes < 8)
        slot_bytes = 8;

    // Unique shm name per benchmark instance to avoid collisions
    char shm_name[64];
    std::snprintf(
        shm_name, sizeof(shm_name), "pipit_bench_shm_%d_%d", static_cast<int>(state.range(0)),
        static_cast<int>(std::chrono::steady_clock::now().time_since_epoch().count() % 100000));

    for ([[maybe_unused]] auto _ : state) {
        pipit::shm::ShmWriter writer;
        if (!writer.init(shm_name, SHM_BENCH_SLOTS, slot_bytes, pipit::net::DTYPE_F32, 0 /* rank */,
                         nullptr /* dims */, static_cast<uint32_t>(N), 0.0 /* rate_hz */,
                         0 /* stable_id_hash */)) {
            state.SkipWithError("Failed to create SHM writer");
            return;
        }

        pipit::shm::ShmReader reader;
        if (!reader.attach(shm_name, SHM_BENCH_SLOTS, slot_bytes, pipit::net::DTYPE_F32,
                           0 /* rank */, nullptr /* dims */, 0.0 /* rate_hz */,
                           0 /* stable_id_hash */)) {
            writer.close();
            state.SkipWithError("Failed to attach SHM reader");
            return;
        }

        std::atomic<bool> stop{false};
        std::atomic<uint64_t> sent_samples{0};
        std::atomic<uint64_t> received_samples{0};

        // Reader thread: tight consume loop → mul(0.5) → discard
        std::thread rx_thread([&] {
            std::vector<float> rx_buf(N);

            while (!stop.load(std::memory_order_acquire)) {
                size_t got = reader.consume(rx_buf.data(), payload_bytes);
                if (got > 0) {
                    size_t samples = got / sizeof(float);
                    // Pipeline: mul(0.5)
                    for (size_t i = 0; i < samples; ++i) {
                        rx_buf[i] *= 0.5f;
                    }
                    benchmark::DoNotOptimize(rx_buf.data());
                    received_samples.fetch_add(samples, std::memory_order_relaxed);
                }
            }

            // Drain remaining
            for (int drain = 0; drain < 1000; ++drain) {
                size_t got = reader.consume(rx_buf.data(), payload_bytes);
                if (got == 0)
                    break;
                size_t samples = got / sizeof(float);
                received_samples.fetch_add(samples, std::memory_order_relaxed);
            }
        });

        // Writer thread: blast for SHM_BENCH_DURATION_MS
        std::thread tx_thread([&] {
            std::vector<float> tx_buf0(N);
            std::vector<float> tx_buf1(N);
            Actor_constant gen{1.0f, N};
            Actor_mul<float> mul1{2.0f, N};
            uint64_t iter = 0;

            while (!stop.load(std::memory_order_relaxed)) {
                gen(nullptr, tx_buf0.data());
                mul1(tx_buf0.data(), tx_buf1.data());
                writer.publish(tx_buf1.data(), payload_bytes, static_cast<uint32_t>(N),
                               pipit::shm::FLAG_FRAME_START | pipit::shm::FLAG_FRAME_END, iter++);
                sent_samples.fetch_add(N, std::memory_order_relaxed);
            }
        });

        // Run for fixed duration
        auto t0 = Clock::now();
        std::this_thread::sleep_for(std::chrono::milliseconds(SHM_BENCH_DURATION_MS));
        stop.store(true, std::memory_order_release);

        tx_thread.join();

        // Brief drain window
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
        rx_thread.join();
        auto t1 = Clock::now();

        writer.close();
        reader.close();

        double elapsed_s = std::chrono::duration<double>(t1 - t0).count();
        uint64_t tx_count = sent_samples.load(std::memory_order_relaxed);
        uint64_t rx_count = received_samples.load(std::memory_order_relaxed);

        state.SetIterationTime(elapsed_s);
        state.SetItemsProcessed(static_cast<int64_t>(rx_count));
        state.SetBytesProcessed(static_cast<int64_t>(rx_count) *
                                static_cast<int64_t>(sizeof(float)));

        state.counters["chunk_size"] = static_cast<double>(N);
        state.counters["slots"] = static_cast<double>(SHM_BENCH_SLOTS);
        state.counters["slot_bytes"] = static_cast<double>(slot_bytes);
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

BENCHMARK(BM_SHM_Loopback)
    ->Arg(64)
    ->Arg(256)
    ->Arg(1024)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

BENCHMARK_MAIN();
