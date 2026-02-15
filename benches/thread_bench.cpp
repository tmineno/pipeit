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

// ── Timer overhead (no latency measurement) ─────────────────────────────────
//
// Same as BM_TimerOverhead but with measure_latency=false.
// Measures improvement from skipping the second Clock::now() call.

static void BM_TimerOverhead_NoLatency(benchmark::State &state) {
    Timer timer(1e9, false); // 1 GHz = always overrun, no latency tracking

    for (auto _ : state) {
        timer.wait();
        timer.overrun();
        benchmark::DoNotOptimize(timer.last_latency());
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_TimerOverhead_NoLatency);

// ── Empty pipeline without stats ────────────────────────────────────────────
//
// Same as BM_EmptyPipeline but with measure_latency=false (stats disabled).

static void BM_EmptyPipeline_NoStats(benchmark::State &state) {
    for (auto _ : state) {
        const int n_ticks = 1000;
        Timer timer(10000.0, false); // 10kHz, no latency measurement
        float dummy_in = 0.0f;
        float dummy_out = 0.0f;

        for (int i = 0; i < n_ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                continue;
            }

            benchmark::DoNotOptimize(dummy_in);
            dummy_out = dummy_in;
            benchmark::DoNotOptimize(dummy_out);
        }

        benchmark::DoNotOptimize(dummy_out);
    }
}
BENCHMARK(BM_EmptyPipeline_NoStats)->Unit(benchmark::kMillisecond);

// ── Empty pipeline with K-factor batching ───────────────────────────────────
//
// K=10: timer fires at 1kHz, 10 actor firings per tick.
// Total firings = 1000 ticks × 10 = 10,000 actor firings.

static void BM_EmptyPipeline_Batched(benchmark::State &state) {
    for (auto _ : state) {
        const int n_ticks = 1000;
        const int k_factor = 10;
        Timer timer(10000.0 / k_factor, false); // 10kHz / K=10 → 1kHz timer
        float dummy_in = 0.0f;
        float dummy_out = 0.0f;

        for (int i = 0; i < n_ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                continue;
            }

            for (int k = 0; k < k_factor; ++k) {
                benchmark::DoNotOptimize(dummy_in);
                dummy_out = dummy_in;
                benchmark::DoNotOptimize(dummy_out);
            }
        }

        benchmark::DoNotOptimize(dummy_out);
    }
}
BENCHMARK(BM_EmptyPipeline_Batched)->Unit(benchmark::kMillisecond);

// ── Thread wake-up with start barrier ───────────────────────────────────────
//
// Measures effective wake-up latency when threads are pre-created and
// wait on an atomic barrier before starting work.

static void BM_ThreadWakeup_Barrier(benchmark::State &state) {
    for (auto _ : state) {
        std::atomic<bool> start{false};
        std::atomic<int64_t> t_ready{0};

        auto t_before = std::chrono::steady_clock::now();

        std::thread worker([&start, &t_ready] {
            // Thread is alive, spin on barrier
            while (!start.load(std::memory_order_acquire)) {
                std::this_thread::yield();
            }
            // Record time when work actually starts
            auto now = std::chrono::steady_clock::now();
            t_ready.store(now.time_since_epoch().count(), std::memory_order_release);
        });

        // Release barrier
        start.store(true, std::memory_order_release);
        worker.join();

        int64_t started = t_ready.load(std::memory_order_acquire);
        int64_t released = std::chrono::steady_clock::now().time_since_epoch().count();
        benchmark::DoNotOptimize(started);
        benchmark::DoNotOptimize(released);
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_ThreadWakeup_Barrier);

// ── High-frequency empty pipeline (1MHz, 10MHz, 100MHz) ────────────────────
//
// Parameterized by frequency. K-factor computed as ceil(freq / 1MHz).
// Each runs 1000 OS ticks with K actor firings per tick.

static void BM_EmptyPipeline_Freq(benchmark::State &state) {
    const double freq_hz = static_cast<double>(state.range(0));
    const double tick_rate_hz = 1000000.0; // 1MHz
    const int k_factor =
        freq_hz <= tick_rate_hz ? 1 : static_cast<int>(std::ceil(freq_hz / tick_rate_hz));
    const int n_ticks = 1000;

    for (auto _ : state) {
        Timer timer(freq_hz / k_factor, false);
        float dummy_in = 0.0f;
        float dummy_out = 0.0f;
        int overruns = 0;

        for (int i = 0; i < n_ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++overruns;
                continue;
            }

            for (int k = 0; k < k_factor; ++k) {
                benchmark::DoNotOptimize(dummy_in);
                dummy_out = dummy_in;
                benchmark::DoNotOptimize(dummy_out);
            }
        }

        state.counters["overruns"] = overruns;
        state.counters["k_factor"] = k_factor;
        state.counters["total_firings"] = (n_ticks - overruns) * k_factor;
    }
}
BENCHMARK(BM_EmptyPipeline_Freq)
    ->Arg(1000000)   // 1MHz: K=1
    ->Arg(10000000)  // 10MHz: K=10
    ->Arg(100000000) // 100MHz: K=100
    ->Unit(benchmark::kMillisecond);

// ── High-frequency with custom tick_rate (10kHz) ────────────────────────────
//
// Same frequencies but with tick_rate=10kHz to test aggressive batching.

static void BM_EmptyPipeline_Freq_TickRate(benchmark::State &state) {
    const double freq_hz = static_cast<double>(state.range(0));
    const double tick_rate_hz = 10000.0; // 10kHz
    const int k_factor =
        freq_hz <= tick_rate_hz ? 1 : static_cast<int>(std::ceil(freq_hz / tick_rate_hz));
    const int n_ticks = 1000;

    for (auto _ : state) {
        Timer timer(freq_hz / k_factor, false);
        float dummy_in = 0.0f;
        float dummy_out = 0.0f;
        int overruns = 0;

        for (int i = 0; i < n_ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++overruns;
                continue;
            }

            for (int k = 0; k < k_factor; ++k) {
                benchmark::DoNotOptimize(dummy_in);
                dummy_out = dummy_in;
                benchmark::DoNotOptimize(dummy_out);
            }
        }

        state.counters["overruns"] = overruns;
        state.counters["k_factor"] = k_factor;
        state.counters["total_firings"] = (n_ticks - overruns) * k_factor;
    }
}
BENCHMARK(BM_EmptyPipeline_Freq_TickRate)
    ->Arg(1000000)   // 1MHz: K=100
    ->Arg(10000000)  // 10MHz: K=1000
    ->Arg(100000000) // 100MHz: K=10000
    ->Unit(benchmark::kMillisecond);

BENCHMARK_MAIN();
