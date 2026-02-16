// Ring buffer stress tests — throughput, contention, and scaling
//
// Tests multi-threaded performance characteristics of pipit::RingBuffer
// across varying reader counts and buffer sizes.

#include <atomic>
#include <benchmark/benchmark.h>
#include <chrono>
#include <cstdlib>
#include <cstring>
#include <pipit.h>
#include <thread>
#include <vector>

#if defined(__linux__)
#include <pthread.h>
#include <sched.h>
#endif

using namespace pipit;

static bool bench_pin_enabled() {
    const char *v = std::getenv("PIPIT_BENCH_PIN");
    return v != nullptr && v[0] != '\0' && v[0] != '0';
}

static void maybe_pin_current_thread(std::size_t cpu_hint) {
#if defined(__linux__)
    if (!bench_pin_enabled())
        return;
    unsigned int n = std::thread::hardware_concurrency();
    if (n == 0)
        return;
    cpu_set_t set;
    CPU_ZERO(&set);
    CPU_SET(static_cast<int>(cpu_hint % n), &set);
    (void)pthread_setaffinity_np(pthread_self(), sizeof(set), &set);
#else
    (void)cpu_hint;
#endif
}

// ── High throughput: sustained write+read ───────────────────────────────────
//
// Writer and reader run in separate threads. Measures sustainable throughput.

static void BM_RingBuffer_Throughput(benchmark::State &state) {
    static constexpr std::size_t CAP = 4096;
    static constexpr std::size_t CHUNK = 64;
    RingBuffer<float, CAP, 1> rb;

    float write_data[CHUNK];
    for (std::size_t i = 0; i < CHUNK; ++i)
        write_data[i] = static_cast<float>(i);

    for (auto _ : state) {
        std::atomic<bool> done{false};
        std::atomic<uint64_t> read_count{0};

        // Reader thread
        std::thread reader([&] {
            float buf[CHUNK];
            while (!done.load(std::memory_order_acquire)) {
                if (rb.read(0, buf, CHUNK)) {
                    read_count.fetch_add(CHUNK, std::memory_order_relaxed);
                }
            }
            // Drain remaining
            float drain[CHUNK];
            while (rb.read(0, drain, CHUNK)) {
                read_count.fetch_add(CHUNK, std::memory_order_relaxed);
            }
        });

        // Writer: push 1M tokens
        uint64_t written = 0;
        const uint64_t target = 1'000'000;
        while (written < target) {
            if (rb.write(write_data, CHUNK)) {
                written += CHUNK;
            }
        }

        done.store(true, std::memory_order_release);
        reader.join();

        state.SetItemsProcessed(written);
        state.SetBytesProcessed(written * sizeof(float));
    }
}
BENCHMARK(BM_RingBuffer_Throughput)->Unit(benchmark::kMillisecond);

// ── Multi-reader contention ─────────────────────────────────────────────────
//
// Template instantiations for different reader counts.
// Each reader runs in its own thread, writer pumps data.

template <std::size_t Readers> static void BM_RingBuffer_Contention(benchmark::State &state) {
    static constexpr std::size_t CAP = 4096;
    static constexpr std::size_t CHUNK = 16;
    static constexpr uint64_t TOKENS = 250'000;

    uint64_t total_written_tokens = 0;
    uint64_t total_reader_tokens = 0;
    uint64_t total_read_success = 0;
    uint64_t total_read_fail = 0;
    uint64_t total_write_fail = 0;

    for (auto _ : state) {
        RingBuffer<float, CAP, Readers> rb;
        float write_data[CHUNK];
        for (std::size_t i = 0; i < CHUNK; ++i)
            write_data[i] = static_cast<float>(i);

        std::atomic<bool> done{false};
        std::vector<std::thread> readers;
        std::atomic<uint64_t> read_success{0};
        std::atomic<uint64_t> read_fail{0};

        // Launch reader threads
        for (std::size_t r = 0; r < Readers; ++r) {
            readers.emplace_back([&rb, &done, &read_success, &read_fail, r] {
                maybe_pin_current_thread(r + 1);
                float buf[CHUNK];
                uint64_t local_success = 0;
                uint64_t local_fail = 0;
                while (!done.load(std::memory_order_acquire)) {
                    if (rb.read(r, buf, CHUNK)) {
                        ++local_success;
                    } else {
                        ++local_fail;
                    }
                }
                // Drain remaining
                while (rb.read(r, buf, CHUNK)) {
                    ++local_success;
                }
                read_success.fetch_add(local_success, std::memory_order_relaxed);
                read_fail.fetch_add(local_fail, std::memory_order_relaxed);
            });
        }

        maybe_pin_current_thread(0);

        // Writer: push tokens
        uint64_t written = 0;
        uint64_t write_fail = 0;
        while (written < TOKENS) {
            if (rb.write(write_data, CHUNK)) {
                written += CHUNK;
            } else {
                ++write_fail;
            }
        }

        done.store(true, std::memory_order_release);
        for (auto &t : readers)
            t.join();

        uint64_t iter_read_success = read_success.load(std::memory_order_relaxed);
        uint64_t iter_read_fail = read_fail.load(std::memory_order_relaxed);

        total_written_tokens += written;
        total_reader_tokens += iter_read_success * CHUNK;
        total_read_success += iter_read_success;
        total_read_fail += iter_read_fail;
        total_write_fail += write_fail;
    }

    state.SetItemsProcessed(total_written_tokens);
    state.SetBytesProcessed(total_written_tokens * sizeof(float));
    state.counters["writer_tokens"] = static_cast<double>(total_written_tokens);
    state.counters["reader_tokens"] = static_cast<double>(total_reader_tokens);
    state.counters["reader_tokens_per_sec"] =
        benchmark::Counter(static_cast<double>(total_reader_tokens), benchmark::Counter::kIsRate);
    state.counters["read_success"] = static_cast<double>(total_read_success);
    state.counters["read_fail"] = static_cast<double>(total_read_fail);
    state.counters["read_fail_pct"] =
        (total_read_success + total_read_fail) > 0
            ? (100.0 * static_cast<double>(total_read_fail) /
               static_cast<double>(total_read_success + total_read_fail))
            : 0.0;
    state.counters["write_fail"] = static_cast<double>(total_write_fail);
    state.counters["write_fail_pct"] =
        ((total_written_tokens / CHUNK) + total_write_fail) > 0
            ? (100.0 * static_cast<double>(total_write_fail) /
               static_cast<double>((total_written_tokens / CHUNK) + total_write_fail))
            : 0.0;
}

BENCHMARK(BM_RingBuffer_Contention<1>)->Name("BM_RingBuffer_Contention/1reader");
BENCHMARK(BM_RingBuffer_Contention<2>)->Name("BM_RingBuffer_Contention/2readers");
BENCHMARK(BM_RingBuffer_Contention<4>)->Name("BM_RingBuffer_Contention/4readers");
BENCHMARK(BM_RingBuffer_Contention<8>)->Name("BM_RingBuffer_Contention/8readers");

// ── Buffer size scaling ─────────────────────────────────────────────────────
//
// Measure write+read round-trip at different buffer capacities.
// Uses single-threaded sequential write/read to isolate buffer effects.

template <std::size_t Cap> static void BM_RingBuffer_SizeScaling(benchmark::State &state) {
    RingBuffer<float, Cap, 1> rb;
    static constexpr std::size_t CHUNK = 16;
    float write_data[CHUNK];
    float read_data[CHUNK];
    for (std::size_t i = 0; i < CHUNK; ++i)
        write_data[i] = static_cast<float>(i);

    for (auto _ : state) {
        rb.write(write_data, CHUNK);
        rb.read(0, read_data, CHUNK);
        benchmark::DoNotOptimize(read_data);
    }

    state.SetItemsProcessed(state.iterations() * CHUNK);
    state.SetBytesProcessed(state.iterations() * CHUNK * sizeof(float));
}

BENCHMARK(BM_RingBuffer_SizeScaling<256>)->Name("BM_RingBuffer_SizeScaling/256");
BENCHMARK(BM_RingBuffer_SizeScaling<1024>)->Name("BM_RingBuffer_SizeScaling/1K");
BENCHMARK(BM_RingBuffer_SizeScaling<4096>)->Name("BM_RingBuffer_SizeScaling/4K");
BENCHMARK(BM_RingBuffer_SizeScaling<16384>)->Name("BM_RingBuffer_SizeScaling/16K");

// ── Chunk size scaling ──────────────────────────────────────────────────────
//
// Measure how chunk (transfer) size affects throughput.

static void BM_RingBuffer_ChunkScaling(benchmark::State &state) {
    static constexpr std::size_t CAP = 65536;
    RingBuffer<float, CAP, 1> rb;
    const int chunk = static_cast<int>(state.range(0));
    std::vector<float> write_data(chunk);
    std::vector<float> read_data(chunk);
    for (int i = 0; i < chunk; ++i)
        write_data[i] = static_cast<float>(i);

    for (auto _ : state) {
        rb.write(write_data.data(), chunk);
        rb.read(0, read_data.data(), chunk);
        benchmark::DoNotOptimize(read_data.data());
    }

    state.SetItemsProcessed(state.iterations() * chunk);
    state.SetBytesProcessed(state.iterations() * chunk * sizeof(float));
}
BENCHMARK(BM_RingBuffer_ChunkScaling)->Arg(16)->Arg(64)->Arg(256);

BENCHMARK_MAIN();
