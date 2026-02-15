// Latency breakdown benchmarks
//
// Detailed latency analysis with percentile tracking:
// - Per-actor firing time (min/avg/max/p99)
// - Timer overhead vs actual work ratio
// - Ring buffer read/write vs compute time
// - Task wake-up to first instruction latency
// - End-to-end latency budget (source -> sink)
//
// Uses custom measurement (same pattern as timer_bench.cpp) for
// per-iteration latency distributions. Not using Google Benchmark
// because we need percentile-level data.

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <numeric>
#include <pipit.h>
#include <std_actors.h>
#include <thread>
#include <vector>

using namespace pipit;
using Clock = std::chrono::steady_clock;
using Nanos = std::chrono::nanoseconds;

// ── Latency statistics ──────────────────────────────────────────────────

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
    if (v.empty())
        return s;
    std::sort(v.begin(), v.end());
    int n = s.count;
    s.min_ns = v[0];
    s.max_ns = v[n - 1];
    s.median_ns = v[n / 2];
    s.p90_ns = v[static_cast<int>(n * 0.90)];
    s.p99_ns = v[static_cast<int>(n * 0.99)];
    s.p999_ns = v[std::min(static_cast<int>(n * 0.999), n - 1)];
    int64_t sum = 0;
    for (auto x : v)
        sum += x;
    s.avg_ns = sum / n;
    return s;
}

static void print_latency(const char *label, const LatencyStats &s) {
    printf("[latency] %-30s n=%-6d min=%-8ld avg=%-8ld med=%-8ld "
           "p90=%-8ld p99=%-8ld p999=%-8ld max=%-8ld ns\n",
           label, s.count, s.min_ns, s.avg_ns, s.median_ns, s.p90_ns, s.p99_ns, s.p999_ns,
           s.max_ns);
}

// ── Helpers ──────────────────────────────────────────────────────────────

static void fill_float(float *buf, int n) {
    for (int i = 0; i < n; ++i)
        buf[i] = static_cast<float>(i) * 0.01f + 0.5f;
}

static void fill_cfloat(cfloat *buf, int n) {
    for (int i = 0; i < n; ++i)
        buf[i] = cfloat(static_cast<float>(i) * 0.01f, 0.0f);
}

// ── 1. Per-actor firing latency ─────────────────────────────────────────

static void measure_actor_latency() {
    printf("\n=== Per-Actor Firing Latency ===\n");
    const int ITERATIONS = 100000;

    // Warmup helper
    auto warmup = [](auto &actor, auto *in, auto *out, int n) {
        for (int i = 0; i < n; ++i)
            actor(in, out);
    };

    // mul(N=64)
    {
        const int N = 64;
        float in[N], out[N];
        fill_float(in, N);
        Actor_mul actor{2.0f, N};
        warmup(actor, in, out, 1000);

        std::vector<int64_t> lat;
        lat.reserve(ITERATIONS);
        for (int i = 0; i < ITERATIONS; ++i) {
            auto t0 = Clock::now();
            actor(in, out);
            auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        auto s = compute_stats(lat);
        print_latency("mul(N=64)", s);
    }

    // add()
    {
        float in[2] = {1.5f, 2.5f}, out[1];
        Actor_add actor{};
        warmup(actor, in, out, 1000);

        std::vector<int64_t> lat;
        lat.reserve(ITERATIONS);
        for (int i = 0; i < ITERATIONS; ++i) {
            auto t0 = Clock::now();
            actor(in, out);
            auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        auto s = compute_stats(lat);
        print_latency("add()", s);
    }

    // fft(N=256)
    {
        const int N = 256;
        float in[N];
        cfloat out[N];
        fill_float(in, N);
        Actor_fft actor{N};
        warmup(actor, in, out, 100);

        std::vector<int64_t> lat;
        lat.reserve(ITERATIONS);
        for (int i = 0; i < ITERATIONS; ++i) {
            auto t0 = Clock::now();
            actor(in, out);
            auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        auto s = compute_stats(lat);
        print_latency("fft(N=256)", s);
    }

    // fir(N=16)
    {
        const int N = 16;
        float coeff[N];
        for (int i = 0; i < N; ++i)
            coeff[i] = 1.0f / N;
        float in[N], out[1];
        fill_float(in, N);
        Actor_fir actor{N, std::span<const float>(coeff, N)};
        warmup(actor, in, out, 1000);

        std::vector<int64_t> lat;
        lat.reserve(ITERATIONS);
        for (int i = 0; i < ITERATIONS; ++i) {
            auto t0 = Clock::now();
            actor(in, out);
            auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        auto s = compute_stats(lat);
        print_latency("fir(N=16)", s);
    }

    // mean(N=64)
    {
        const int N = 64;
        float in[N], out[1];
        fill_float(in, N);
        Actor_mean actor{N};
        warmup(actor, in, out, 1000);

        std::vector<int64_t> lat;
        lat.reserve(ITERATIONS);
        for (int i = 0; i < ITERATIONS; ++i) {
            auto t0 = Clock::now();
            actor(in, out);
            auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        auto s = compute_stats(lat);
        print_latency("mean(N=64)", s);
    }

    // c2r(N=256)
    {
        const int N = 256;
        cfloat in[N];
        float out[N];
        fill_cfloat(in, N);
        Actor_c2r actor{N};
        warmup(actor, in, out, 100);

        std::vector<int64_t> lat;
        lat.reserve(ITERATIONS);
        for (int i = 0; i < ITERATIONS; ++i) {
            auto t0 = Clock::now();
            actor(in, out);
            auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        auto s = compute_stats(lat);
        print_latency("c2r(N=256)", s);
    }

    // rms(N=64)
    {
        const int N = 64;
        float in[N], out[1];
        fill_float(in, N);
        Actor_rms actor{N};
        warmup(actor, in, out, 1000);

        std::vector<int64_t> lat;
        lat.reserve(ITERATIONS);
        for (int i = 0; i < ITERATIONS; ++i) {
            auto t0 = Clock::now();
            actor(in, out);
            auto t1 = Clock::now();
            lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        }
        auto s = compute_stats(lat);
        print_latency("rms(N=64)", s);
    }
}

// ── 2. Timer overhead vs actual work ────────────────────────────────────

static void measure_timer_vs_work() {
    printf("\n=== Timer Overhead vs Actual Work ===\n");

    const int TICKS = 10000;
    const double FREQ = 10000.0; // 10kHz

    Timer timer(FREQ);
    std::vector<int64_t> timer_lat;
    std::vector<int64_t> work_lat;
    timer_lat.reserve(TICKS);
    work_lat.reserve(TICKS);

    const int N = 64;
    float in[N], out[N];
    fill_float(in, N);
    Actor_mul actor{2.0f, N};

    for (int i = 0; i < TICKS; ++i) {
        auto t0 = Clock::now();
        timer.wait();
        auto t1 = Clock::now();
        timer_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());

        auto t2 = Clock::now();
        actor(in, out);
        auto t3 = Clock::now();
        work_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());
    }

    auto ts = compute_stats(timer_lat);
    auto ws = compute_stats(work_lat);
    print_latency("timer.wait() @10kHz", ts);
    print_latency("actor_work(mul,64)", ws);

    if (ts.avg_ns + ws.avg_ns > 0) {
        printf("[latency] overhead_ratio: timer / (timer + work) = %.2f%%\n",
               100.0 * ts.avg_ns / (ts.avg_ns + ws.avg_ns));
    }
}

// ── 3. Ring buffer read/write vs compute ────────────────────────────────

static void measure_buffer_vs_compute() {
    printf("\n=== Ring Buffer vs Compute Time ===\n");

    const int ITERATIONS = 100000;
    const int CHUNK = 64;

    RingBuffer<float, 4096, 1> rb;
    float write_data[CHUNK], read_data[CHUNK], out[CHUNK];
    fill_float(write_data, CHUNK);
    Actor_mul actor{2.0f, CHUNK};

    std::vector<int64_t> write_lat, read_lat, compute_lat;
    write_lat.reserve(ITERATIONS);
    read_lat.reserve(ITERATIONS);
    compute_lat.reserve(ITERATIONS);

    for (int i = 0; i < ITERATIONS; ++i) {
        auto t0 = Clock::now();
        rb.write(write_data, CHUNK);
        auto t1 = Clock::now();
        write_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());

        auto t2 = Clock::now();
        rb.read(0, read_data, CHUNK);
        auto t3 = Clock::now();
        read_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());

        auto t4 = Clock::now();
        actor(read_data, out);
        auto t5 = Clock::now();
        compute_lat.push_back(std::chrono::duration_cast<Nanos>(t5 - t4).count());
    }

    auto ws = compute_stats(write_lat);
    auto rs = compute_stats(read_lat);
    auto cs = compute_stats(compute_lat);
    print_latency("rb.write(64)", ws);
    print_latency("rb.read(64)", rs);
    print_latency("actor.compute(64)", cs);

    int64_t total = ws.avg_ns + rs.avg_ns + cs.avg_ns;
    if (total > 0) {
        printf("[latency] budget: write=%.1f%% read=%.1f%% compute=%.1f%%\n",
               100.0 * ws.avg_ns / total, 100.0 * rs.avg_ns / total, 100.0 * cs.avg_ns / total);
    }
}

// ── 4. Task wake-up to first instruction ────────────────────────────────

static void measure_wakeup_latency() {
    printf("\n=== Task Wake-up to First Instruction ===\n");

    const int ITERATIONS = 1000;
    std::vector<int64_t> latencies;
    latencies.reserve(ITERATIONS);

    for (int i = 0; i < ITERATIONS; ++i) {
        std::atomic<int64_t> t_start{0};

        auto t_before = Clock::now();

        std::thread worker([&t_start] {
            auto now = Clock::now();
            t_start.store(now.time_since_epoch().count(), std::memory_order_release);
        });

        worker.join();
        int64_t started = t_start.load(std::memory_order_acquire);
        int64_t launched = t_before.time_since_epoch().count();
        latencies.push_back(started - launched);
    }

    auto s = compute_stats(latencies);
    print_latency("thread_wakeup", s);
}

// ── 5. End-to-end latency budget ────────────────────────────────────────

static void measure_e2e_latency() {
    printf("\n=== End-to-End Latency Budget ===\n");
    printf("Pipeline: mul(2.0) -> fir(5-tap) -> mean(5)\n");

    const int ITERATIONS = 100000;

    Actor_mul mul_actor{2.0f, 1};
    float fir_coeff[] = {0.1f, 0.2f, 0.4f, 0.2f, 0.1f};
    Actor_fir fir_actor{5, std::span<const float>(fir_coeff, 5)};
    Actor_mean mean_actor{5};

    // Separate per-stage timing
    std::vector<int64_t> mul_lat, fir_lat, mean_lat, total_lat;
    mul_lat.reserve(ITERATIONS);
    fir_lat.reserve(ITERATIONS);
    mean_lat.reserve(ITERATIONS);
    total_lat.reserve(ITERATIONS);

    for (int i = 0; i < ITERATIONS; ++i) {
        float val = 1.0f;

        auto t0 = Clock::now();

        // Stage 1: mul
        float mul_out;
        mul_actor(&val, &mul_out);
        auto t1 = Clock::now();

        // Stage 2: FIR (needs 5 samples)
        float fir_in[5] = {mul_out, mul_out, mul_out, mul_out, mul_out};
        float fir_out;
        fir_actor(fir_in, &fir_out);
        auto t2 = Clock::now();

        // Stage 3: mean (needs 5 samples)
        float mean_in[5] = {fir_out, fir_out, fir_out, fir_out, fir_out};
        float mean_out;
        mean_actor(mean_in, &mean_out);
        auto t3 = Clock::now();

        mul_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());
        fir_lat.push_back(std::chrono::duration_cast<Nanos>(t2 - t1).count());
        mean_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());
        total_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t0).count());
    }

    auto ms = compute_stats(mul_lat);
    auto fs = compute_stats(fir_lat);
    auto ns = compute_stats(mean_lat);
    auto ts = compute_stats(total_lat);
    print_latency("  stage: mul(1)", ms);
    print_latency("  stage: fir(5)", fs);
    print_latency("  stage: mean(5)", ns);
    print_latency("  total: e2e", ts);

    if (ts.avg_ns > 0) {
        printf("[latency] e2e_budget: mul=%.1f%% fir=%.1f%% mean=%.1f%%\n",
               100.0 * ms.avg_ns / ts.avg_ns, 100.0 * fs.avg_ns / ts.avg_ns,
               100.0 * ns.avg_ns / ts.avg_ns);
    }
}

// ── 6. Timer overhead vs work (batched K=10) ────────────────────────────

static void measure_timer_vs_work_batched() {
    printf("\n=== Timer Overhead vs Work (Batched K=10) ===\n");
    printf("Timer fires at 1kHz, 10 actor firings per tick (total effective: 10kHz)\n");

    const int TICKS = 1000;
    const int K = 10;
    const double FREQ = 1000.0; // 1kHz timer (10kHz / K=10)

    Timer timer(FREQ, false); // No latency measurement
    std::vector<int64_t> timer_lat;
    std::vector<int64_t> work_lat;
    timer_lat.reserve(TICKS);
    work_lat.reserve(TICKS);

    const int N = 64;
    float in[N], out[N];
    fill_float(in, N);
    Actor_mul actor{2.0f, N};

    for (int i = 0; i < TICKS; ++i) {
        auto t0 = Clock::now();
        timer.wait();
        auto t1 = Clock::now();
        timer_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());

        auto t2 = Clock::now();
        for (int k = 0; k < K; ++k) {
            actor(in, out);
        }
        auto t3 = Clock::now();
        work_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());
    }

    auto ts = compute_stats(timer_lat);
    auto ws = compute_stats(work_lat);
    print_latency("timer.wait() @1kHz", ts);
    print_latency("actor_work(mul,64)x10", ws);

    if (ts.avg_ns + ws.avg_ns > 0) {
        printf("[latency] overhead_ratio: timer / (timer + work) = %.2f%%\n",
               100.0 * ts.avg_ns / (ts.avg_ns + ws.avg_ns));
        printf("[latency] per_firing_overhead: timer_avg / K = %.1f ns\n",
               static_cast<double>(ts.avg_ns) / K);
    }
}

// ── 7. Timer overhead at high frequencies (1MHz, 10MHz, 100MHz) ──────────

static void measure_timer_vs_work_freq() {
    printf("\n=== Timer Overhead vs Work at High Frequencies ===\n");

    struct FreqSpec {
        double freq_hz;
        int k;
        int ticks;
        const char *label;
    };

    FreqSpec specs[] = {
        {1000000.0, 1, 1000, "1MHz_K1"},
        {10000000.0, 10, 1000, "10MHz_K10"},
        {100000000.0, 100, 1000, "100MHz_K100"},
    };

    const int N = 64;
    float in[N], out[N];
    fill_float(in, N);
    Actor_mul actor{2.0f, N};

    for (auto &spec : specs) {
        printf("\n--- %s (timer @ %.0f Hz, K=%d) ---\n", spec.label, spec.freq_hz / spec.k, spec.k);

        Timer timer(spec.freq_hz / spec.k, true);
        std::vector<int64_t> timer_lat;
        std::vector<int64_t> work_lat;
        timer_lat.reserve(spec.ticks);
        work_lat.reserve(spec.ticks);
        int overruns = 0;

        for (int i = 0; i < spec.ticks; ++i) {
            auto t0 = Clock::now();
            timer.wait();
            auto t1 = Clock::now();
            if (timer.overrun()) {
                ++overruns;
            }
            timer_lat.push_back(std::chrono::duration_cast<Nanos>(t1 - t0).count());

            auto t2 = Clock::now();
            for (int k = 0; k < spec.k; ++k) {
                actor(in, out);
            }
            auto t3 = Clock::now();
            work_lat.push_back(std::chrono::duration_cast<Nanos>(t3 - t2).count());
        }

        auto ts = compute_stats(timer_lat);
        auto ws = compute_stats(work_lat);
        print_latency("  timer.wait()", ts);
        print_latency("  actor_work", ws);

        if (ts.avg_ns + ws.avg_ns > 0) {
            printf("[latency] overhead_ratio: %.2f%%, overruns=%d/%d\n",
                   100.0 * ts.avg_ns / (ts.avg_ns + ws.avg_ns), overruns, spec.ticks);
            printf("[latency] per_firing: timer=%.1f ns, work=%.1f ns\n",
                   static_cast<double>(ts.avg_ns) / spec.k,
                   static_cast<double>(ws.avg_ns) / spec.k);
        }
    }
}

// ── Main ─────────────────────────────────────────────────────────────────

int main() {
    printf("=== Pipit Latency Breakdown Benchmarks ===\n");

    measure_actor_latency();
    measure_timer_vs_work();
    measure_buffer_vs_compute();
    measure_wakeup_latency();
    measure_e2e_latency();
    measure_timer_vs_work_batched();
    measure_timer_vs_work_freq();

    printf("\n=== Done ===\n");
    return 0;
}
