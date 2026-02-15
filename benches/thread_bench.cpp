// Task scheduling overhead benchmarks
//
// Measures thread creation/join cost, context switch overhead,
// empty pipeline framework cost, and task scaling characteristics.

#include <atomic>
#include <benchmark/benchmark.h>
#include <chrono>
#include <pipit.h>
#include <thread>
#include <vector>

using namespace pipit;

// ── Thread creation/join cost ───────────────────────────────────────────────

static void BM_ThreadCreateJoin(benchmark::State &state) {
    for (auto _ : state) {
        std::thread t([] { benchmark::DoNotOptimize(0); });
        t.join();
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_ThreadCreateJoin);

// ── Context switch overhead ─────────────────────────────────────────────────
//
// Two threads ping-pong via atomic flag, measure round-trip.

static void BM_ContextSwitch(benchmark::State &state) {
    std::atomic<int> flag{0};
    std::atomic<bool> done{false};
    const int rounds = 1000;

    for (auto _ : state) {
        done.store(false, std::memory_order_release);
        flag.store(0, std::memory_order_release);

        std::thread other([&] {
            for (int i = 0; i < rounds; ++i) {
                while (flag.load(std::memory_order_acquire) != 1) {
                    std::this_thread::yield();
                }
                flag.store(0, std::memory_order_release);
            }
        });

        for (int i = 0; i < rounds; ++i) {
            flag.store(1, std::memory_order_release);
            while (flag.load(std::memory_order_acquire) != 0) {
                std::this_thread::yield();
            }
        }

        other.join();
        state.SetItemsProcessed(rounds * 2);
    }
}
BENCHMARK(BM_ContextSwitch)->Unit(benchmark::kMillisecond);

// ── Empty pipeline overhead ─────────────────────────────────────────────────
//
// Minimal timer+actor loop measuring framework overhead per tick.
// Runs a single task with no-op actor at 10kHz for ~100ms.

static void BM_EmptyPipeline(benchmark::State &state) {
    for (auto _ : state) {
        const int n_ticks = 1000;
        Timer timer(10000.0); // 10kHz
        TaskStats stats;
        float dummy_in = 0.0f;
        float dummy_out = 0.0f;

        for (int i = 0; i < n_ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                stats.record_miss();
                continue;
            }
            stats.record_tick(timer.last_latency());

            // Minimal actor work (simulate framework overhead)
            benchmark::DoNotOptimize(dummy_in);
            dummy_out = dummy_in;
            benchmark::DoNotOptimize(dummy_out);
        }

        benchmark::DoNotOptimize(stats.ticks);
        benchmark::DoNotOptimize(stats.missed);
    }
}
BENCHMARK(BM_EmptyPipeline)->Unit(benchmark::kMillisecond);

// ── Task scaling ────────────────────────────────────────────────────────────
//
// Launch N threads, each running a timer loop for a fixed number of ticks.
// Measures aggregate throughput and per-thread overhead as N scales.

static void BM_TaskScaling(benchmark::State &state) {
    const int n_threads = static_cast<int>(state.range(0));
    const int ticks_per_thread = 100;
    const double freq = 10000.0; // 10kHz

    for (auto _ : state) {
        std::atomic<uint64_t> total_ticks{0};
        std::atomic<uint64_t> total_missed{0};
        std::vector<std::thread> threads;

        for (int t = 0; t < n_threads; ++t) {
            threads.emplace_back([&, freq] {
                Timer timer(freq);
                TaskStats stats;

                for (int i = 0; i < ticks_per_thread; ++i) {
                    timer.wait();
                    if (timer.overrun()) {
                        stats.record_miss();
                    } else {
                        stats.record_tick(timer.last_latency());
                    }
                }

                total_ticks.fetch_add(stats.ticks, std::memory_order_relaxed);
                total_missed.fetch_add(stats.missed, std::memory_order_relaxed);
            });
        }

        for (auto &t : threads)
            t.join();

        state.SetItemsProcessed(total_ticks.load(std::memory_order_relaxed) +
                                total_missed.load(std::memory_order_relaxed));
    }
}
BENCHMARK(BM_TaskScaling)
    ->Arg(1)
    ->Arg(2)
    ->Arg(4)
    ->Arg(8)
    ->Arg(16)
    ->Arg(32)
    ->Unit(benchmark::kMillisecond);

// ── Timer overhead (no sleep) ───────────────────────────────────────────────
//
// Measures pure Timer object overhead without sleeping.
// Creates timer, calls wait+overrun+last_latency in a tight loop.

static void BM_TimerOverhead(benchmark::State &state) {
    // Use a very high frequency so timer never sleeps (always overruns)
    Timer timer(1e9); // 1 GHz = always overrun

    for (auto _ : state) {
        timer.wait();
        timer.overrun();
        benchmark::DoNotOptimize(timer.last_latency());
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_TimerOverhead);

BENCHMARK_MAIN();
