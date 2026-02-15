// Runtime benchmarks for libpipit
// Benchmark key runtime primitives: RingBuffer, Timer, TaskStats

#include <atomic>
#include <benchmark/benchmark.h>
#include <pipit.h>
#include <thread>

using namespace pipit;

// Benchmark: RingBuffer write performance
static void BM_RingBuffer_Write(benchmark::State &state) {
    RingBuffer<float, 1024, 1> rb;
    float data[64];
    for (int i = 0; i < 64; ++i)
        data[i] = static_cast<float>(i);

    // Pre-consume to make room
    float dummy[64];
    size_t write_count = 0;
    for (auto _ : state) {
        if (!rb.write(data, 64)) {
            // Buffer full, read to make space
            rb.read(dummy, 64);
        } else {
            ++write_count;
        }
    }

    state.SetItemsProcessed(write_count * 64);
    state.SetBytesProcessed(write_count * 64 * sizeof(float));
}
BENCHMARK(BM_RingBuffer_Write);

// Benchmark: RingBuffer read performance
static void BM_RingBuffer_Read(benchmark::State &state) {
    RingBuffer<float, 1024, 1> rb;
    float write_data[64];
    float read_data[64];
    for (int i = 0; i < 64; ++i)
        write_data[i] = static_cast<float>(i);

    // Pre-fill buffer
    for (int i = 0; i < 10; ++i) {
        rb.write(write_data, 64);
    }

    for (auto _ : state) {
        if (!rb.read(0, read_data, 64)) {
            // Refill when empty
            for (int i = 0; i < 10; ++i) {
                rb.write(write_data, 64);
            }
        }
    }

    state.SetItemsProcessed(state.iterations() * 64);
    state.SetBytesProcessed(state.iterations() * 64 * sizeof(float));
}
BENCHMARK(BM_RingBuffer_Read);

// Benchmark: RingBuffer write/read round-trip
static void BM_RingBuffer_RoundTrip(benchmark::State &state) {
    RingBuffer<float, 1024, 1> rb;
    float write_data[32];
    float read_data[32];
    for (int i = 0; i < 32; ++i)
        write_data[i] = static_cast<float>(i);

    for (auto _ : state) {
        rb.write(write_data, 32);
        rb.read(0, read_data, 32);
        benchmark::DoNotOptimize(read_data);
    }

    state.SetItemsProcessed(state.iterations() * 32);
}
BENCHMARK(BM_RingBuffer_RoundTrip);

// Benchmark: RingBuffer multi-reader (2 readers)
static void BM_RingBuffer_MultiReader(benchmark::State &state) {
    RingBuffer<float, 1024, 2> rb;
    float write_data[16];
    float read_data[16];
    for (int i = 0; i < 16; ++i)
        write_data[i] = static_cast<float>(i);

    for (auto _ : state) {
        rb.write(write_data, 16);
        rb.read(0, read_data, 16); // Reader 0
        rb.read(1, read_data, 16); // Reader 1
        benchmark::DoNotOptimize(read_data);
    }

    state.SetItemsProcessed(state.iterations() * 16);
}
BENCHMARK(BM_RingBuffer_MultiReader);

// Benchmark: Timer tick accuracy (1 kHz)
static void BM_Timer_Tick_1kHz(benchmark::State &state) {
    Timer timer(1000.0); // 1 kHz
    std::atomic<bool> stop{false};

    for (auto _ : state) {
        timer.wait();
        timer.overrun(); // Check for overruns
        benchmark::DoNotOptimize(timer.last_latency());
    }
}
BENCHMARK(BM_Timer_Tick_1kHz);

// Benchmark: Timer tick accuracy (10 kHz)
static void BM_Timer_Tick_10kHz(benchmark::State &state) {
    Timer timer(10000.0); // 10 kHz

    for (auto _ : state) {
        timer.wait();
        timer.overrun();
        benchmark::DoNotOptimize(timer.last_latency());
    }
}
BENCHMARK(BM_Timer_Tick_10kHz);

// Benchmark: TaskStats record operations
static void BM_TaskStats_Record(benchmark::State &state) {
    TaskStats stats;

    for (auto _ : state) {
        stats.record_tick(std::chrono::nanoseconds(1000)); // 1000ns latency
        stats.record_miss();
        benchmark::DoNotOptimize(stats.avg_latency_ns());
    }

    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_TaskStats_Record);

// Benchmark: Atomic parameter load (simulating runtime param access)
static void BM_AtomicParam_Load(benchmark::State &state) {
    std::atomic<double> param{1.0};

    for (auto _ : state) {
        double val = param.load(std::memory_order_acquire);
        benchmark::DoNotOptimize(val);
    }

    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_AtomicParam_Load);

// Benchmark: Atomic parameter store (simulating runtime param update)
static void BM_AtomicParam_Store(benchmark::State &state) {
    std::atomic<double> param{1.0};
    double value = 2.5;

    for (auto _ : state) {
        param.store(value, std::memory_order_release);
    }

    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_AtomicParam_Store);

BENCHMARK_MAIN();
