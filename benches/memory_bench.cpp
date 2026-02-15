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
#include <cstring>
#include <pipit.h>
#include <thread>
#include <vector>

using namespace pipit;

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

    for (auto _ : state) {
        RingBuffer<float, CAP, Readers> rb;
        float write_data[CHUNK];
        for (std::size_t i = 0; i < CHUNK; ++i)
            write_data[i] = static_cast<float>(i);

        std::atomic<bool> done{false};
        std::vector<std::thread> readers;
        std::atomic<uint64_t> total_reads{0};

        for (std::size_t r = 0; r < Readers; ++r) {
            readers.emplace_back([&rb, &done, &total_reads, r] {
                float buf[CHUNK];
                uint64_t local_reads = 0;
                while (!done.load(std::memory_order_acquire)) {
                    if (rb.read(r, buf, CHUNK))
                        ++local_reads;
                }
                while (rb.read(r, buf, CHUNK))
                    ++local_reads;
                total_reads.fetch_add(local_reads, std::memory_order_relaxed);
            });
        }

        uint64_t written = 0;
        const uint64_t target = 100'000;
        while (written < target) {
            if (rb.write(write_data, CHUNK))
                written += CHUNK;
        }

        done.store(true, std::memory_order_release);
        for (auto &t : readers)
            t.join();

        state.SetItemsProcessed(written);
    }
}
BENCHMARK(BM_Memory_FalseSharing<1>)->Name("BM_Memory_FalseSharing/1reader");
BENCHMARK(BM_Memory_FalseSharing<2>)->Name("BM_Memory_FalseSharing/2readers");
BENCHMARK(BM_Memory_FalseSharing<4>)->Name("BM_Memory_FalseSharing/4readers");
BENCHMARK(BM_Memory_FalseSharing<8>)->Name("BM_Memory_FalseSharing/8readers");

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
