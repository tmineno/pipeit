// Memory subsystem benchmarks
//
// Measures memory characteristics of Pipit runtime components:
// - Per-task memory footprint estimation
// - Cache line utilization in RingBuffer
// - False sharing detection between reader tails
// - Memory bandwidth saturation
// - Page fault impact (cold vs warm allocation)

#include <atomic>
#include <benchmark/benchmark.h>
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

// ── Memory footprint estimation ─────────────────────────────────────────
//
// Reports sizeof() for each core component via benchmark counters.
// Visible in JSON output as custom counters.

static void BM_Memory_Footprint(benchmark::State &state) {
    for (auto _ : state) {
        benchmark::DoNotOptimize(sizeof(RingBuffer<float, 1024, 1>));
        benchmark::DoNotOptimize(sizeof(RingBuffer<float, 4096, 4>));
        benchmark::DoNotOptimize(sizeof(Timer));
        benchmark::DoNotOptimize(sizeof(TaskStats));
    }
    state.counters["RB_f_1024_1r"] = sizeof(RingBuffer<float, 1024, 1>);
    state.counters["RB_f_4096_4r"] = sizeof(RingBuffer<float, 4096, 4>);
    state.counters["RB_f_16384_1r"] = sizeof(RingBuffer<float, 16384, 1>);
    state.counters["RB_f_4096_8r"] = sizeof(RingBuffer<float, 4096, 8>);
    state.counters["RB_f_4096_16r"] = sizeof(RingBuffer<float, 4096, 16>);
    state.counters["RB_cf_1024_1r"] = sizeof(RingBuffer<cfloat, 1024, 1>);
    state.counters["Timer"] = sizeof(Timer);
    state.counters["TaskStats"] = sizeof(TaskStats);
}
BENCHMARK(BM_Memory_Footprint);

// ── Cache line utilization ──────────────────────────────────────────────
//
// Measure throughput at different chunk sizes through RingBuffer.
// Small chunks waste cache lines; larger chunks utilize them fully.

static void BM_Memory_CacheLineUtil(benchmark::State &state) {
    static constexpr std::size_t CAP = 65536;
    RingBuffer<float, CAP, 1> rb;
    const int chunk = static_cast<int>(state.range(0));
    std::vector<float> data(chunk, 1.0f);
    std::vector<float> out(chunk);

    for (auto _ : state) {
        rb.write(data.data(), chunk);
        rb.read(0, out.data(), chunk);
        benchmark::DoNotOptimize(out.data());
    }
    state.SetBytesProcessed(state.iterations() * chunk * sizeof(float) * 2);
    state.SetItemsProcessed(state.iterations() * chunk);
}
BENCHMARK(BM_Memory_CacheLineUtil)->Arg(1)->Arg(4)->Arg(16)->Arg(64)->Arg(256)->Arg(1024);

// ── False sharing detection ─────────────────────────────────────────────
//
// Multi-reader contention benchmark. With alignas(64) on tails_ in
// RingBuffer, false sharing should not occur. This benchmark validates
// that: if per-reader throughput degrades significantly as reader count
// grows beyond contention effects, false sharing may be present.

template <std::size_t Readers> static void BM_Memory_FalseSharing(benchmark::State &state) {
    static constexpr std::size_t CAP = 4096;
    static constexpr std::size_t CHUNK = 8;

    uint64_t total_written_tokens = 0;
    uint64_t total_reader_tokens = 0;
    uint64_t total_read_success = 0;
    uint64_t total_read_fail = 0;
    uint64_t total_write_slow_path = 0;
    uint64_t total_write_fail = 0;

    for (auto _ : state) {
        RingBuffer<float, CAP, Readers> rb;
        rb.debug_reset_write_counters();
        float write_data[CHUNK];
        for (std::size_t i = 0; i < CHUNK; ++i)
            write_data[i] = static_cast<float>(i);

        std::atomic<bool> done{false};
        std::vector<std::thread> readers;
        std::atomic<uint64_t> read_success{0};
        std::atomic<uint64_t> read_fail{0};

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
                while (rb.read(r, buf, CHUNK)) {
                    ++local_success;
                }
                read_success.fetch_add(local_success, std::memory_order_relaxed);
                read_fail.fetch_add(local_fail, std::memory_order_relaxed);
            });
        }

        maybe_pin_current_thread(0);

        uint64_t written = 0;
        const uint64_t target = 100'000;
        while (written < target) {
            if (rb.write(write_data, CHUNK))
                written += CHUNK;
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
        total_write_slow_path += rb.debug_write_slow_path_count();
        total_write_fail += rb.debug_write_fail_count();
    }

    state.SetItemsProcessed(total_written_tokens);
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
    state.counters["write_slow_path"] = static_cast<double>(total_write_slow_path);
    state.counters["write_fail"] = static_cast<double>(total_write_fail);
    state.counters["write_fail_pct"] =
        ((total_written_tokens / CHUNK) + total_write_fail) > 0
            ? (100.0 * static_cast<double>(total_write_fail) /
               static_cast<double>((total_written_tokens / CHUNK) + total_write_fail))
            : 0.0;
}
BENCHMARK(BM_Memory_FalseSharing<1>)->Name("BM_Memory_FalseSharing/1reader");
BENCHMARK(BM_Memory_FalseSharing<2>)->Name("BM_Memory_FalseSharing/2readers");
BENCHMARK(BM_Memory_FalseSharing<4>)->Name("BM_Memory_FalseSharing/4readers");
BENCHMARK(BM_Memory_FalseSharing<8>)->Name("BM_Memory_FalseSharing/8readers");
BENCHMARK(BM_Memory_FalseSharing<16>)->Name("BM_Memory_FalseSharing/16readers");

// ── Memory bandwidth saturation ─────────────────────────────────────────
//
// Raw memcpy at increasing buffer sizes. Throughput should plateau
// at memory bandwidth limit. Uses memcpy to measure raw DRAM bandwidth
// independent of RingBuffer overhead.

static void BM_Memory_Bandwidth(benchmark::State &state) {
    const int buffer_kb = static_cast<int>(state.range(0));
    const std::size_t n_floats = static_cast<std::size_t>(buffer_kb) * 1024 / sizeof(float);
    std::vector<float> src(n_floats, 1.0f);
    std::vector<float> dst(n_floats);

    for (auto _ : state) {
        std::memcpy(dst.data(), src.data(), n_floats * sizeof(float));
        benchmark::DoNotOptimize(dst.data());
        benchmark::ClobberMemory();
    }
    state.SetBytesProcessed(state.iterations() * static_cast<int64_t>(n_floats * sizeof(float)));
}
BENCHMARK(BM_Memory_Bandwidth)
    ->Arg(4)
    ->Arg(16)
    ->Arg(64)
    ->Arg(256)
    ->Arg(1024)
    ->Arg(4096)
    ->Arg(16384)
    ->Unit(benchmark::kMicrosecond);

// ── Page fault impact ────────────────────────────────────────────────────
//
// Compare fresh allocation (cold pages, triggers page faults) vs
// pre-touched allocation (warm pages, no faults).

static void BM_Memory_PageFault_Cold(benchmark::State &state) {
    const std::size_t size = 65536;

    for (auto _ : state) {
        // Fresh allocation — OS defers page mapping until first touch
        auto *buf = new float[size];
        for (std::size_t i = 0; i < size; i += 1024)
            buf[i] = 1.0f;
        benchmark::DoNotOptimize(buf);
        delete[] buf;
    }
    state.SetBytesProcessed(state.iterations() * size * sizeof(float));
}
BENCHMARK(BM_Memory_PageFault_Cold);

static void BM_Memory_PageFault_Warm(benchmark::State &state) {
    const std::size_t size = 65536;
    auto *buf = new float[size];
    // Pre-touch all pages
    std::memset(buf, 0, size * sizeof(float));

    for (auto _ : state) {
        for (std::size_t i = 0; i < size; i += 1024)
            buf[i] = 1.0f;
        benchmark::DoNotOptimize(buf);
    }
    delete[] buf;
    state.SetBytesProcessed(state.iterations() * size * sizeof(float));
}
BENCHMARK(BM_Memory_PageFault_Warm);

BENCHMARK_MAIN();
