#include <benchmark/benchmark.h>

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <pipit.h>
#include <thread>
#include <vector>

using namespace pipit;
using Clock = std::chrono::steady_clock;
using Nanos = std::chrono::nanoseconds;

struct LatencyStats {
    int64_t min_ns = 0;
    int64_t max_ns = 0;
    int64_t median_ns = 0;
    int64_t avg_ns = 0;
    int64_t p90_ns = 0;
    int64_t p99_ns = 0;
    int64_t p999_ns = 0;
    int64_t overruns = 0;
    int64_t total_ticks = 0;
};

static LatencyStats compute_stats(std::vector<int64_t> &latencies, int64_t overruns) {
    LatencyStats s{};
    s.total_ticks = static_cast<int64_t>(latencies.size());
    s.overruns = overruns;
    if (latencies.empty()) {
        return s;
    }

    std::sort(latencies.begin(), latencies.end());
    const int n = static_cast<int>(latencies.size());

    s.min_ns = latencies[0];
    s.max_ns = latencies[n - 1];
    s.median_ns = latencies[n / 2];
    s.p90_ns = latencies[static_cast<int>(n * 0.90)];
    s.p99_ns = latencies[static_cast<int>(n * 0.99)];
    s.p999_ns = latencies[std::min(static_cast<int>(n * 0.999), n - 1)];

    int64_t sum = 0;
    for (auto v : latencies) {
        sum += v;
    }
    s.avg_ns = sum / n;
    return s;
}

static LatencyStats measure_timer_distribution(double freq_hz, int ticks, int64_t spin_ns = 0) {
    Timer timer(freq_hz, true, spin_ns);
    std::vector<int64_t> latencies;
    latencies.reserve(ticks);
    int64_t overruns = 0;

    for (int i = 0; i < ticks; ++i) {
        timer.wait();
        if (timer.overrun()) {
            ++overruns;
        }
        latencies.push_back(timer.last_latency().count());
    }

    return compute_stats(latencies, overruns);
}

static void set_latency_counters(benchmark::State &state, const LatencyStats &s, double freq_hz) {
    state.counters["freq_hz"] = freq_hz;
    state.counters["ticks"] = static_cast<double>(s.total_ticks);
    state.counters["overruns"] = static_cast<double>(s.overruns);
    state.counters["min_ns"] = static_cast<double>(s.min_ns);
    state.counters["avg_ns"] = static_cast<double>(s.avg_ns);
    state.counters["median_ns"] = static_cast<double>(s.median_ns);
    state.counters["p90_ns"] = static_cast<double>(s.p90_ns);
    state.counters["p99_ns"] = static_cast<double>(s.p99_ns);
    state.counters["p999_ns"] = static_cast<double>(s.p999_ns);
    state.counters["max_ns"] = static_cast<double>(s.max_ns);
}

static void BM_Timer_FrequencySweep(benchmark::State &state) {
    const double freq_hz = static_cast<double>(state.range(0));
    const int ticks = static_cast<int>(state.range(1));

    for ([[maybe_unused]] auto _ : state) {
        const auto t0 = Clock::now();
        const auto stats = measure_timer_distribution(freq_hz, ticks);
        const auto t1 = Clock::now();

        set_latency_counters(state, stats, freq_hz);
        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
        benchmark::DoNotOptimize(stats.avg_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(ticks) * state.iterations());
}

BENCHMARK(BM_Timer_FrequencySweep)
    ->Args({1000, 1000})
    ->Args({10000, 2000})
    ->Args({48000, 2000})
    ->Args({100000, 2000})
    ->Args({1000000, 1000})
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_Timer_JitterHistogram(benchmark::State &state) {
    constexpr double kFreqHz = 10000.0;
    constexpr int kTicks = 5000;

    for ([[maybe_unused]] auto _ : state) {
        const auto t0 = Clock::now();
        Timer timer(kFreqHz);
        std::vector<int64_t> latencies;
        latencies.reserve(kTicks);
        int64_t overruns = 0;

        for (int i = 0; i < kTicks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++overruns;
            }
            latencies.push_back(timer.last_latency().count());
        }

        const auto stats = compute_stats(latencies, overruns);
        const auto t1 = Clock::now();

        set_latency_counters(state, stats, kFreqHz);

        const int buckets[] = {100, 1000, 10000, 100000, 1000000, 10000000};
        int prev = 0;
        int idx = 0;
        for (int b : buckets) {
            int count = 0;
            for (auto v : latencies) {
                if (v >= prev && v < b) {
                    ++count;
                }
            }
            state.counters["hist_bucket_" + std::to_string(idx)] = static_cast<double>(count);
            prev = b;
            ++idx;
        }
        int overflow = 0;
        for (auto v : latencies) {
            if (v >= buckets[5]) {
                ++overflow;
            }
        }
        state.counters["hist_bucket_overflow"] = static_cast<double>(overflow);

        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
        benchmark::DoNotOptimize(stats.p99_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kTicks) * state.iterations());
}

BENCHMARK(BM_Timer_JitterHistogram)->UseManualTime()->Iterations(1)->Unit(benchmark::kMillisecond);

static void BM_Timer_OverrunRecovery(benchmark::State &state) {
    constexpr double kFreqHz = 1000.0;

    for ([[maybe_unused]] auto _ : state) {
        const auto t0 = Clock::now();

        Timer timer(kFreqHz);
        for (int i = 0; i < 10; ++i) {
            timer.wait();
        }

        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        timer.wait();

        const int64_t missed = timer.missed_count();
        const int64_t overrun_detected = timer.overrun() ? 1 : 0;
        const int64_t overrun_latency_ns = timer.last_latency().count();

        timer.reset_phase();

        std::vector<int64_t> post_latencies;
        post_latencies.reserve(100);
        int64_t post_overruns = 0;
        for (int i = 0; i < 100; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++post_overruns;
            }
            post_latencies.push_back(timer.last_latency().count());
        }

        const auto post_stats = compute_stats(post_latencies, post_overruns);
        const auto t1 = Clock::now();

        set_latency_counters(state, post_stats, kFreqHz);
        state.counters["recovery_missed_count"] = static_cast<double>(missed);
        state.counters["recovery_overrun_detected"] = static_cast<double>(overrun_detected);
        state.counters["recovery_overrun_latency_ns"] = static_cast<double>(overrun_latency_ns);

        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
        benchmark::DoNotOptimize(post_stats.avg_ns);
    }

    state.SetItemsProcessed(100 * state.iterations());
}

BENCHMARK(BM_Timer_OverrunRecovery)->UseManualTime()->Iterations(1)->Unit(benchmark::kMillisecond);

static void BM_Timer_WakeupLatency(benchmark::State &state) {
    constexpr double kFreqHz = 1000.0;
    constexpr int kTicks = 1000;

    for ([[maybe_unused]] auto _ : state) {
        const auto t0 = Clock::now();
        const auto stats = measure_timer_distribution(kFreqHz, kTicks);
        const auto t1 = Clock::now();

        set_latency_counters(state, stats, kFreqHz);
        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
        benchmark::DoNotOptimize(stats.median_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kTicks) * state.iterations());
}

BENCHMARK(BM_Timer_WakeupLatency)->UseManualTime()->Iterations(1)->Unit(benchmark::kMillisecond);

static void BM_Timer_JitterSpin(benchmark::State &state) {
    constexpr double kFreqHz = 10000.0;
    constexpr int kTicks = 2000;
    const int64_t spin_ns = state.range(0);

    for ([[maybe_unused]] auto _ : state) {
        const auto t0 = Clock::now();
        const auto stats = measure_timer_distribution(kFreqHz, kTicks, spin_ns);
        const auto t1 = Clock::now();

        set_latency_counters(state, stats, kFreqHz);
        state.counters["spin_ns"] = static_cast<double>(spin_ns);
        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
        benchmark::DoNotOptimize(stats.p99_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kTicks) * state.iterations());
}

BENCHMARK(BM_Timer_JitterSpin)
    ->Arg(0)
    ->Arg(10000)
    ->Arg(50000)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_Timer_BatchVsSingle(benchmark::State &state) {
    constexpr int total_firings = 20000;
    const int k = static_cast<int>(state.range(0));
    const int ticks = total_firings / k;
    const double timer_freq_hz = 10000.0 / static_cast<double>(k);

    for ([[maybe_unused]] auto _ : state) {
        Timer timer(timer_freq_hz, false);
        int64_t overruns = 0;

        const auto t0 = Clock::now();
        for (int i = 0; i < ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++overruns;
                continue;
            }
            for (int j = 0; j < k; ++j) {
                volatile float x = 1.0f;
                benchmark::DoNotOptimize(x);
            }
        }
        const auto t1 = Clock::now();

        const double elapsed_s = std::chrono::duration<double>(t1 - t0).count();
        const double elapsed_ns = elapsed_s * 1e9;

        state.counters["k_factor"] = static_cast<double>(k);
        state.counters["ticks"] = static_cast<double>(ticks);
        state.counters["overruns"] = static_cast<double>(overruns);
        state.counters["per_firing_ns"] = elapsed_ns / static_cast<double>(total_firings);
        state.SetIterationTime(elapsed_s);
    }

    state.SetItemsProcessed(static_cast<int64_t>(total_firings) * state.iterations());
}

BENCHMARK(BM_Timer_BatchVsSingle)
    ->Arg(1)
    ->Arg(10)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_Timer_HighFreqBatched(benchmark::State &state) {
    const double freq_hz = static_cast<double>(state.range(0));
    const int k = static_cast<int>(state.range(1));
    const int ticks = static_cast<int>(state.range(2));

    for ([[maybe_unused]] auto _ : state) {
        Timer timer(freq_hz / static_cast<double>(k), true);
        std::vector<int64_t> timer_lat;
        timer_lat.reserve(ticks);
        int64_t overruns = 0;

        const auto t0 = Clock::now();
        for (int i = 0; i < ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++overruns;
            }
            timer_lat.push_back(timer.last_latency().count());
        }
        const auto t1 = Clock::now();

        const auto stats = compute_stats(timer_lat, overruns);
        set_latency_counters(state, stats, freq_hz / static_cast<double>(k));
        state.counters["k_factor"] = static_cast<double>(k);
        state.counters["effective_firings"] = static_cast<double>((ticks - overruns) * k);

        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
        benchmark::DoNotOptimize(stats.avg_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(ticks) * state.iterations());
}

BENCHMARK(BM_Timer_HighFreqBatched)
    ->Args({100000, 1, 2000})
    ->Args({1000000, 10, 2000})
    ->Args({10000000, 100, 2000})
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

BENCHMARK_MAIN();
