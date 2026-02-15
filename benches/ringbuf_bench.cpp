// Ring buffer stress tests — throughput, contention, and scaling
//
// Tests multi-threaded performance characteristics of pipit::RingBuffer
// across varying reader counts and buffer sizes.

#include <atomic>
#include <benchmark/benchmark.h>
#include <chrono>
#include <cstring>
#include <pipit.h>
#include <thread>
#include <vector>

using namespace pipit;

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
    static constexpr uint64_t TOKENS = 100'000;

    for (auto _ : state) {
        RingBuffer<float, CAP, Readers> rb;
        float write_data[CHUNK];
        for (std::size_t i = 0; i < CHUNK; ++i)
            write_data[i] = static_cast<float>(i);

        std::atomic<bool> done{false};
        std::vector<std::thread> readers;

        // Launch reader threads
        for (std::size_t r = 0; r < Readers; ++r) {
            readers.emplace_back([&rb, &done, r] {
                float buf[CHUNK];
                while (!done.load(std::memory_order_acquire)) {
                    rb.read(r, buf, CHUNK);
                }
                // Drain remaining
                while (rb.read(r, buf, CHUNK)) {
                }
            });
        }

        // Writer: push tokens
        uint64_t written = 0;
        while (written < TOKENS) {
            if (rb.write(write_data, CHUNK)) {
                written += CHUNK;
            }
        }

        done.store(true, std::memory_order_release);
        for (auto &t : readers)
            t.join();

        state.SetItemsProcessed(written);
    }
}

BENCHMARK(BM_RingBuffer_Contention<2>)->Name("BM_RingBuffer_Contention/2readers");
BENCHMARK(BM_RingBuffer_Contention<4>)->Name("BM_RingBuffer_Contention/4readers");
BENCHMARK(BM_RingBuffer_Contention<8>)->Name("BM_RingBuffer_Contention/8readers");
BENCHMARK(BM_RingBuffer_Contention<16>)->Name("BM_RingBuffer_Contention/16readers");

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

BENCHMARK(BM_RingBuffer_SizeScaling<64>)->Name("BM_RingBuffer_SizeScaling/64");
BENCHMARK(BM_RingBuffer_SizeScaling<256>)->Name("BM_RingBuffer_SizeScaling/256");
BENCHMARK(BM_RingBuffer_SizeScaling<1024>)->Name("BM_RingBuffer_SizeScaling/1K");
BENCHMARK(BM_RingBuffer_SizeScaling<4096>)->Name("BM_RingBuffer_SizeScaling/4K");
BENCHMARK(BM_RingBuffer_SizeScaling<16384>)->Name("BM_RingBuffer_SizeScaling/16K");
BENCHMARK(BM_RingBuffer_SizeScaling<65536>)->Name("BM_RingBuffer_SizeScaling/64K");

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
BENCHMARK(BM_RingBuffer_ChunkScaling)->Arg(1)->Arg(4)->Arg(16)->Arg(64)->Arg(256)->Arg(1024);

BENCHMARK_MAIN();
