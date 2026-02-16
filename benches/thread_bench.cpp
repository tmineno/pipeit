// Scheduler-focused runtime benchmarks.
//
// KPI coverage:
// - deadline miss ratio under representative clock rates
// - scaling behavior across concurrent tasks
// - K-factor/tick_rate amortization effect on per-firing overhead

#include <benchmark/benchmark.h>

#include <chrono>
#include <cmath>
#include <cstdint>
#include <pipit.h>
#include <thread>
#include <vector>

using namespace pipit;
using Clock = std::chrono::steady_clock;

static void BM_TaskDeadline(benchmark::State &state) {
    const double freq_hz = static_cast<double>(state.range(0));
    const int ticks = static_cast<int>(state.range(1));

    uint64_t total_ticks = 0;
    uint64_t total_missed = 0;

    for (auto _ : state) {
        Timer timer(freq_hz);
        TaskStats stats;
        float in = 1.0f;
        float out = 0.0f;

        const auto t0 = Clock::now();
        for (int i = 0; i < ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                stats.record_miss();
                continue;
            }
            stats.record_tick(timer.last_latency());

            // Minimal actor-like work to keep benchmark close to runtime semantics.
            benchmark::DoNotOptimize(in);
            out = in;
            benchmark::DoNotOptimize(out);
        }
        const auto t1 = Clock::now();

        total_ticks += stats.ticks;
        total_missed += stats.missed;
        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
    }

    const double total = static_cast<double>(total_ticks + total_missed);
    state.counters["clock_hz"] = freq_hz;
    state.counters["ticks"] = static_cast<double>(total_ticks);
    state.counters["missed"] = static_cast<double>(total_missed);
    state.counters["miss_rate_pct"] =
        total > 0.0 ? (100.0 * static_cast<double>(total_missed) / total) : 0.0;
    state.SetItemsProcessed(static_cast<int64_t>(total_ticks + total_missed));
}

BENCHMARK(BM_TaskDeadline)
    ->Args({1000, 2000})
    ->Args({10000, 3000})
    ->Args({48000, 3000})
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_TaskScaling(benchmark::State &state) {
    const int n_threads = static_cast<int>(state.range(0));
    const int ticks_per_thread = 500;
    const double freq_hz = 10000.0;

    uint64_t total_ticks = 0;
    uint64_t total_missed = 0;

    for (auto _ : state) {
        std::vector<std::thread> threads;
        std::vector<TaskStats> stats(static_cast<size_t>(n_threads));

        const auto t0 = Clock::now();
        for (int t = 0; t < n_threads; ++t) {
            threads.emplace_back([&, t] {
                Timer timer(freq_hz);
                auto &local = stats[static_cast<size_t>(t)];
                for (int i = 0; i < ticks_per_thread; ++i) {
                    timer.wait();
                    if (timer.overrun()) {
                        local.record_miss();
                    } else {
                        local.record_tick(timer.last_latency());
                    }
                }
            });
        }

        for (auto &th : threads) {
            th.join();
        }
        const auto t1 = Clock::now();

        for (const auto &s : stats) {
            total_ticks += s.ticks;
            total_missed += s.missed;
        }

        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
    }

    const double total = static_cast<double>(total_ticks + total_missed);
    state.counters["threads"] = static_cast<double>(n_threads);
    state.counters["ticks"] = static_cast<double>(total_ticks);
    state.counters["missed"] = static_cast<double>(total_missed);
    state.counters["miss_rate_pct"] =
        total > 0.0 ? (100.0 * static_cast<double>(total_missed) / total) : 0.0;
    state.SetItemsProcessed(static_cast<int64_t>(total_ticks + total_missed));
}

BENCHMARK(BM_TaskScaling)
    ->Arg(1)
    ->Arg(2)
    ->Arg(4)
    ->Arg(8)
    ->Arg(16)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_KFactorBatching(benchmark::State &state) {
    const int k_factor = static_cast<int>(state.range(0));
    const int total_firings = 100000;
    const int ticks = total_firings / k_factor;
    const double effective_hz = 100000.0;
    const double timer_hz = effective_hz / static_cast<double>(k_factor);

    uint64_t total_overruns = 0;

    for (auto _ : state) {
        Timer timer(timer_hz, false);
        float in = 1.0f;
        float out = 0.0f;

        const auto t0 = Clock::now();
        for (int i = 0; i < ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++total_overruns;
                continue;
            }
            for (int k = 0; k < k_factor; ++k) {
                benchmark::DoNotOptimize(in);
                out = in;
                benchmark::DoNotOptimize(out);
            }
        }
        const auto t1 = Clock::now();

        const double elapsed_ns = std::chrono::duration<double, std::nano>(t1 - t0).count();
        state.counters["per_firing_ns"] = elapsed_ns / static_cast<double>(total_firings);
        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
    }

    state.counters["effective_hz"] = effective_hz;
    state.counters["timer_hz"] = timer_hz;
    state.counters["k_factor"] = static_cast<double>(k_factor);
    state.counters["overruns"] = static_cast<double>(total_overruns);
    state.SetItemsProcessed(static_cast<int64_t>(total_firings) * state.iterations());
}

BENCHMARK(BM_KFactorBatching)
    ->Arg(1)
    ->Arg(10)
    ->Arg(100)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_HighFreqTickRate(benchmark::State &state) {
    const double effective_hz = static_cast<double>(state.range(0));
    const double tick_rate_hz = static_cast<double>(state.range(1));
    const int ticks = static_cast<int>(state.range(2));
    const int k_factor =
        effective_hz <= tick_rate_hz ? 1 : static_cast<int>(std::ceil(effective_hz / tick_rate_hz));
    const double timer_hz = effective_hz / static_cast<double>(k_factor);

    for (auto _ : state) {
        Timer timer(timer_hz, false);
        uint64_t overruns = 0;

        const auto t0 = Clock::now();
        for (int i = 0; i < ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++overruns;
            }
        }
        const auto t1 = Clock::now();

        state.counters["effective_hz"] = effective_hz;
        state.counters["tick_rate_hz"] = tick_rate_hz;
        state.counters["timer_hz"] = timer_hz;
        state.counters["k_factor"] = static_cast<double>(k_factor);
        state.counters["overruns"] = static_cast<double>(overruns);
        state.counters["effective_firings"] = static_cast<double>((ticks - overruns) * k_factor);
        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
    }

    state.SetItemsProcessed(static_cast<int64_t>(ticks) * state.iterations());
}

BENCHMARK(BM_HighFreqTickRate)
    ->Args({100000, 100000, 2000})
    ->Args({1000000, 100000, 2000})
    ->Args({10000000, 100000, 2000})
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

BENCHMARK_MAIN();
