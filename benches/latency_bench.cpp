#include <benchmark/benchmark.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <pipit.h>
#include <std_actors.h>
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
    int count = 0;
};

static LatencyStats compute_stats(std::vector<int64_t> &v) {
    LatencyStats s{};
    s.count = static_cast<int>(v.size());
    if (v.empty()) {
        return s;
    }

    std::sort(v.begin(), v.end());
    const int n = s.count;
    s.min_ns = v[0];
    s.max_ns = v[n - 1];
    s.median_ns = v[n / 2];
    s.p90_ns = v[static_cast<int>(n * 0.90)];
    s.p99_ns = v[static_cast<int>(n * 0.99)];
    s.p999_ns = v[std::min(static_cast<int>(n * 0.999), n - 1)];

    int64_t sum = 0;
    for (auto x : v) {
        sum += x;
    }
    s.avg_ns = sum / n;
    return s;
}

static void fill_float(float *buf, int n) {
    for (int i = 0; i < n; ++i) {
        buf[i] = static_cast<float>(i) * 0.01f + 0.5f;
    }
}

static void fill_cfloat(cfloat *buf, int n) {
    for (int i = 0; i < n; ++i) {
        buf[i] = cfloat(static_cast<float>(i) * 0.01f, 0.0f);
    }
}

static void set_latency_counters(benchmark::State &state, const LatencyStats &s) {
    state.counters["n"] = static_cast<double>(s.count);
    state.counters["min_ns"] = static_cast<double>(s.min_ns);
    state.counters["avg_ns"] = static_cast<double>(s.avg_ns);
    state.counters["median_ns"] = static_cast<double>(s.median_ns);
    state.counters["p90_ns"] = static_cast<double>(s.p90_ns);
    state.counters["p99_ns"] = static_cast<double>(s.p99_ns);
    state.counters["p999_ns"] = static_cast<double>(s.p999_ns);
    state.counters["max_ns"] = static_cast<double>(s.max_ns);
}

static LatencyStats measure_actor_case(int actor_case, int iterations) {
    std::vector<int64_t> lat;
    lat.reserve(iterations);

    switch (actor_case) {
    case 0: {
        const int N = 64;
        float in[N], out[N];
        fill_float(in, N);
        Actor_mul actor{2.0f, N};
        for (int i = 0; i < 1000; ++i) {
            actor(in, out);
        }
        for (int i = 0; i < iterations; ++i) {
            const auto t0 = Clock::now();
            actor(in, out);
            const auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        break;
    }
    case 1: {
        float in[2] = {1.5f, 2.5f};
        float out[1];
        Actor_add actor{};
        for (int i = 0; i < 1000; ++i) {
            actor(in, out);
        }
        for (int i = 0; i < iterations; ++i) {
            const auto t0 = Clock::now();
            actor(in, out);
            const auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        break;
    }
    case 2: {
        const int N = 256;
        float in[N];
        cfloat out[N];
        fill_float(in, N);
        Actor_fft actor{N};
        for (int i = 0; i < 100; ++i) {
            actor(in, out);
        }
        for (int i = 0; i < iterations; ++i) {
            const auto t0 = Clock::now();
            actor(in, out);
            const auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        break;
    }
    case 3: {
        const int N = 16;
        float coeff[N];
        for (int i = 0; i < N; ++i) {
            coeff[i] = 1.0f / N;
        }
        float in[N], out[1];
        fill_float(in, N);
        Actor_fir actor{N, std::span<const float>(coeff, N)};
        for (int i = 0; i < 1000; ++i) {
            actor(in, out);
        }
        for (int i = 0; i < iterations; ++i) {
            const auto t0 = Clock::now();
            actor(in, out);
            const auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        break;
    }
    case 4: {
        const int N = 64;
        float in[N], out[1];
        fill_float(in, N);
        Actor_mean actor{N};
        for (int i = 0; i < 1000; ++i) {
            actor(in, out);
        }
        for (int i = 0; i < iterations; ++i) {
            const auto t0 = Clock::now();
            actor(in, out);
            const auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        break;
    }
    case 5: {
        const int N = 256;
        cfloat in[N];
        float out[N];
        fill_cfloat(in, N);
        Actor_c2r actor{N};
        for (int i = 0; i < 100; ++i) {
            actor(in, out);
        }
        for (int i = 0; i < iterations; ++i) {
            const auto t0 = Clock::now();
            actor(in, out);
            const auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        break;
    }
    default: {
        const int N = 64;
        float in[N], out[1];
        fill_float(in, N);
        Actor_rms actor{N};
        for (int i = 0; i < 1000; ++i) {
            actor(in, out);
        }
        for (int i = 0; i < iterations; ++i) {
            const auto t0 = Clock::now();
            actor(in, out);
            const auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        break;
    }
    }

    return compute_stats(lat);
}

static void BM_Latency_ActorFiring(benchmark::State &state) {
    const int actor_case = static_cast<int>(state.range(0));
    constexpr int kIterations = 100000;

    for ([[maybe_unused]] auto _ : state) {
        const auto t0 = Clock::now();
        const auto s = measure_actor_case(actor_case, kIterations);
        const auto t1 = Clock::now();

        set_latency_counters(state, s);
        state.SetIterationTime(std::chrono::duration<double>(t1 - t0).count());
        benchmark::DoNotOptimize(s.avg_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kIterations) * state.iterations());
}

BENCHMARK(BM_Latency_ActorFiring)
    ->Arg(0)
    ->Arg(1)
    ->Arg(2)
    ->Arg(3)
    ->Arg(4)
    ->Arg(5)
    ->Arg(6)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_Latency_TimerVsWork(benchmark::State &state) {
    constexpr int kTicks = 10000;
    constexpr double kFreq = 10000.0;

    for ([[maybe_unused]] auto _ : state) {
        Timer timer(kFreq);
        std::vector<int64_t> timer_lat;
        std::vector<int64_t> work_lat;
        timer_lat.reserve(kTicks);
        work_lat.reserve(kTicks);

        const int N = 64;
        float in[N], out[N];
        fill_float(in, N);
        Actor_mul actor{2.0f, N};

        const auto w0 = Clock::now();
        for (int i = 0; i < kTicks; ++i) {
            const auto t0 = Clock::now();
            timer.wait();
            const auto t1 = Clock::now();
            timer_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());

            const auto t2 = Clock::now();
            actor(in, out);
            const auto t3 = Clock::now();
            work_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());
        }
        const auto w1 = Clock::now();

        auto ts = compute_stats(timer_lat);
        auto ws = compute_stats(work_lat);

        set_latency_counters(state, ts);
        state.counters["work_avg_ns"] = static_cast<double>(ws.avg_ns);
        const double denom = static_cast<double>(ts.avg_ns + ws.avg_ns);
        state.counters["overhead_ratio_pct"] = denom > 0.0 ? (100.0 * ts.avg_ns / denom) : 0.0;
        state.SetIterationTime(std::chrono::duration<double>(w1 - w0).count());
        benchmark::DoNotOptimize(ts.avg_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kTicks) * state.iterations());
}

BENCHMARK(BM_Latency_TimerVsWork)->UseManualTime()->Iterations(1)->Unit(benchmark::kMillisecond);

static void BM_Latency_BufferVsCompute(benchmark::State &state) {
    constexpr int kIterations = 100000;
    constexpr int kChunk = 64;

    for ([[maybe_unused]] auto _ : state) {
        RingBuffer<float, 4096, 1> rb;
        float write_data[kChunk], read_data[kChunk], out[kChunk];
        fill_float(write_data, kChunk);
        Actor_mul actor{2.0f, kChunk};

        std::vector<int64_t> write_lat, read_lat, compute_lat;
        write_lat.reserve(kIterations);
        read_lat.reserve(kIterations);
        compute_lat.reserve(kIterations);

        const auto w0 = Clock::now();
        for (int i = 0; i < kIterations; ++i) {
            const auto t0 = Clock::now();
            benchmark::DoNotOptimize(rb.write(write_data, kChunk));
            const auto t1 = Clock::now();
            write_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());

            const auto t2 = Clock::now();
            benchmark::DoNotOptimize(rb.read(0, read_data, kChunk));
            const auto t3 = Clock::now();
            read_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());

            const auto t4 = Clock::now();
            actor(read_data, out);
            const auto t5 = Clock::now();
            compute_lat.push_back(std::chrono::duration_cast<Nanos>(t5 - t4).count());
        }
        const auto w1 = Clock::now();

        auto ws = compute_stats(write_lat);
        auto rs = compute_stats(read_lat);
        auto cs = compute_stats(compute_lat);

        state.counters["write_avg_ns"] = static_cast<double>(ws.avg_ns);
        state.counters["read_avg_ns"] = static_cast<double>(rs.avg_ns);
        state.counters["compute_avg_ns"] = static_cast<double>(cs.avg_ns);

        const double total = static_cast<double>(ws.avg_ns + rs.avg_ns + cs.avg_ns);
        state.counters["write_budget_pct"] = total > 0.0 ? (100.0 * ws.avg_ns / total) : 0.0;
        state.counters["read_budget_pct"] = total > 0.0 ? (100.0 * rs.avg_ns / total) : 0.0;
        state.counters["compute_budget_pct"] = total > 0.0 ? (100.0 * cs.avg_ns / total) : 0.0;

        state.SetIterationTime(std::chrono::duration<double>(w1 - w0).count());
        benchmark::DoNotOptimize(total);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kIterations) * state.iterations());
}

BENCHMARK(BM_Latency_BufferVsCompute)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_Latency_Wakeup(benchmark::State &state) {
    constexpr int kIterations = 1000;

    for ([[maybe_unused]] auto _ : state) {
        std::vector<int64_t> latencies;
        latencies.reserve(kIterations);

        const auto w0 = Clock::now();
        for (int i = 0; i < kIterations; ++i) {
            std::atomic<int64_t> t_start{0};
            const auto t_before = Clock::now();

            std::thread worker([&t_start] {
                const auto now = Clock::now();
                t_start.store(now.time_since_epoch().count(), std::memory_order_release);
            });

            worker.join();
            const int64_t started = t_start.load(std::memory_order_acquire);
            const int64_t launched = t_before.time_since_epoch().count();
            latencies.push_back(started - launched);
        }
        const auto w1 = Clock::now();

        auto s = compute_stats(latencies);
        set_latency_counters(state, s);
        state.SetIterationTime(std::chrono::duration<double>(w1 - w0).count());
        benchmark::DoNotOptimize(s.p99_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kIterations) * state.iterations());
}

BENCHMARK(BM_Latency_Wakeup)->UseManualTime()->Iterations(1)->Unit(benchmark::kMillisecond);

static void BM_Latency_E2E(benchmark::State &state) {
    constexpr int kIterations = 100000;

    for ([[maybe_unused]] auto _ : state) {
        Actor_mul mul_actor{2.0f, 1};
        float fir_coeff[] = {0.1f, 0.2f, 0.4f, 0.2f, 0.1f};
        Actor_fir fir_actor{5, std::span<const float>(fir_coeff, 5)};
        Actor_mean mean_actor{5};

        std::vector<int64_t> mul_lat, fir_lat, mean_lat, total_lat;
        mul_lat.reserve(kIterations);
        fir_lat.reserve(kIterations);
        mean_lat.reserve(kIterations);
        total_lat.reserve(kIterations);

        const auto w0 = Clock::now();
        for (int i = 0; i < kIterations; ++i) {
            float val = 1.0f;

            const auto t0 = Clock::now();
            float mul_out;
            mul_actor(&val, &mul_out);
            const auto t1 = Clock::now();

            float fir_in[5] = {mul_out, mul_out, mul_out, mul_out, mul_out};
            float fir_out;
            fir_actor(fir_in, &fir_out);
            const auto t2 = Clock::now();

            float mean_in[5] = {fir_out, fir_out, fir_out, fir_out, fir_out};
            float mean_out;
            mean_actor(mean_in, &mean_out);
            const auto t3 = Clock::now();

            mul_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
            fir_lat.push_back(std::chrono::duration_cast<Nanos>(t2 - t1).count());
            mean_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());
            total_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t0).count());
            benchmark::DoNotOptimize(mean_out);
        }
        const auto w1 = Clock::now();

        auto ms = compute_stats(mul_lat);
        auto fs = compute_stats(fir_lat);
        auto ns = compute_stats(mean_lat);
        auto ts = compute_stats(total_lat);

        set_latency_counters(state, ts);
        state.counters["mul_avg_ns"] = static_cast<double>(ms.avg_ns);
        state.counters["fir_avg_ns"] = static_cast<double>(fs.avg_ns);
        state.counters["mean_avg_ns"] = static_cast<double>(ns.avg_ns);

        const double total_avg = static_cast<double>(ts.avg_ns);
        state.counters["mul_budget_pct"] = total_avg > 0.0 ? (100.0 * ms.avg_ns / total_avg) : 0.0;
        state.counters["fir_budget_pct"] = total_avg > 0.0 ? (100.0 * fs.avg_ns / total_avg) : 0.0;
        state.counters["mean_budget_pct"] = total_avg > 0.0 ? (100.0 * ns.avg_ns / total_avg) : 0.0;

        state.SetIterationTime(std::chrono::duration<double>(w1 - w0).count());
        benchmark::DoNotOptimize(ts.p99_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kIterations) * state.iterations());
}

BENCHMARK(BM_Latency_E2E)->UseManualTime()->Iterations(1)->Unit(benchmark::kMillisecond);

static void BM_Latency_TimerVsWork_Batched(benchmark::State &state) {
    constexpr int kTicks = 1000;
    constexpr int k = 10;
    constexpr double kFreq = 1000.0;

    for ([[maybe_unused]] auto _ : state) {
        Timer timer(kFreq, false);
        std::vector<int64_t> timer_lat;
        std::vector<int64_t> work_lat;
        timer_lat.reserve(kTicks);
        work_lat.reserve(kTicks);

        const int N = 64;
        float in[N], out[N];
        fill_float(in, N);
        Actor_mul actor{2.0f, N};

        const auto w0 = Clock::now();
        for (int i = 0; i < kTicks; ++i) {
            const auto t0 = Clock::now();
            timer.wait();
            const auto t1 = Clock::now();
            timer_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());

            const auto t2 = Clock::now();
            for (int j = 0; j < k; ++j) {
                actor(in, out);
            }
            const auto t3 = Clock::now();
            work_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());
        }
        const auto w1 = Clock::now();

        auto ts = compute_stats(timer_lat);
        auto ws = compute_stats(work_lat);

        state.counters["timer_avg_ns"] = static_cast<double>(ts.avg_ns);
        state.counters["work_avg_ns"] = static_cast<double>(ws.avg_ns);
        const double denom = static_cast<double>(ts.avg_ns + ws.avg_ns);
        state.counters["overhead_ratio_pct"] = denom > 0.0 ? (100.0 * ts.avg_ns / denom) : 0.0;
        state.counters["per_firing_overhead_ns"] = static_cast<double>(ts.avg_ns) / k;

        state.SetIterationTime(std::chrono::duration<double>(w1 - w0).count());
        benchmark::DoNotOptimize(ts.avg_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(kTicks * k) * state.iterations());
}

BENCHMARK(BM_Latency_TimerVsWork_Batched)
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

static void BM_Latency_TimerVsWork_HighFreq(benchmark::State &state) {
    const double freq_hz = static_cast<double>(state.range(0));
    const int k = static_cast<int>(state.range(1));
    const int ticks = static_cast<int>(state.range(2));

    for ([[maybe_unused]] auto _ : state) {
        Timer timer(freq_hz / static_cast<double>(k), true);
        std::vector<int64_t> timer_lat;
        std::vector<int64_t> work_lat;
        timer_lat.reserve(ticks);
        work_lat.reserve(ticks);

        const int N = 64;
        float in[N], out[N];
        fill_float(in, N);
        Actor_mul actor{2.0f, N};

        int64_t overruns = 0;
        const auto w0 = Clock::now();
        for (int i = 0; i < ticks; ++i) {
            const auto t0 = Clock::now();
            timer.wait();
            const auto t1 = Clock::now();
            if (timer.overrun()) {
                ++overruns;
            }
            timer_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());

            const auto t2 = Clock::now();
            for (int j = 0; j < k; ++j) {
                actor(in, out);
            }
            const auto t3 = Clock::now();
            work_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());
        }
        const auto w1 = Clock::now();

        auto ts = compute_stats(timer_lat);
        auto ws = compute_stats(work_lat);

        state.counters["freq_hz"] = freq_hz;
        state.counters["k_factor"] = static_cast<double>(k);
        state.counters["ticks"] = static_cast<double>(ticks);
        state.counters["overruns"] = static_cast<double>(overruns);
        state.counters["timer_avg_ns"] = static_cast<double>(ts.avg_ns);
        state.counters["work_avg_ns"] = static_cast<double>(ws.avg_ns);

        const double denom = static_cast<double>(ts.avg_ns + ws.avg_ns);
        state.counters["overhead_ratio_pct"] = denom > 0.0 ? (100.0 * ts.avg_ns / denom) : 0.0;
        state.counters["timer_per_firing_ns"] = static_cast<double>(ts.avg_ns) / k;
        state.counters["work_per_firing_ns"] = static_cast<double>(ws.avg_ns) / k;

        state.SetIterationTime(std::chrono::duration<double>(w1 - w0).count());
        benchmark::DoNotOptimize(ws.p99_ns);
    }

    state.SetItemsProcessed(static_cast<int64_t>(ticks * k) * state.iterations());
}

BENCHMARK(BM_Latency_TimerVsWork_HighFreq)
    ->Args({1000000, 1, 1000})
    ->Args({10000000, 10, 1000})
    ->Args({100000000, 100, 1000})
    ->UseManualTime()
    ->Iterations(1)
    ->Unit(benchmark::kMillisecond);

BENCHMARK_MAIN();
