// Timer precision benchmarks — frequency sweep, jitter, overrun recovery
//
// Not using Google Benchmark (timers have real-time constraints that conflict
// with benchmark harness timing). Custom measurement with structured output.

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <numeric>
#include <pipit.h>
#include <thread>
#include <vector>

using namespace pipit;
using Clock = std::chrono::steady_clock;
using Nanos = std::chrono::nanoseconds;

// ── Helpers ─────────────────────────────────────────────────────────────────

struct LatencyStats {
    int64_t min_ns;
    int64_t max_ns;
    int64_t median_ns;
    int64_t avg_ns;
    int64_t p90_ns;
    int64_t p99_ns;
    int64_t p999_ns;
    int overruns;
    int total_ticks;
};

static LatencyStats compute_stats(std::vector<int64_t> &latencies, int overruns) {
    LatencyStats s{};
    s.total_ticks = static_cast<int>(latencies.size());
    s.overruns = overruns;
    if (latencies.empty())
        return s;

    std::sort(latencies.begin(), latencies.end());
    int n = static_cast<int>(latencies.size());

    s.min_ns = latencies[0];
    s.max_ns = latencies[n - 1];
    s.median_ns = latencies[n / 2];
    s.p90_ns = latencies[static_cast<int>(n * 0.90)];
    s.p99_ns = latencies[static_cast<int>(n * 0.99)];
    s.p999_ns = latencies[std::min(static_cast<int>(n * 0.999), n - 1)];

    int64_t sum = 0;
    for (auto v : latencies)
        sum += v;
    s.avg_ns = sum / n;

    return s;
}

static void print_stats(const char *label, double freq_hz, const LatencyStats &s) {
    printf("[timer_bench] %-20s freq=%-10.0f ticks=%-6d overruns=%-4d "
           "min=%-8ld avg=%-8ld median=%-8ld p90=%-8ld p99=%-8ld p99.9=%-8ld max=%-8ld ns\n",
           label, freq_hz, s.total_ticks, s.overruns, s.min_ns, s.avg_ns, s.median_ns, s.p90_ns,
           s.p99_ns, s.p999_ns, s.max_ns);
}

// ── Frequency sweep ─────────────────────────────────────────────────────────
//
// Timer at 1Hz, 10Hz, 100Hz, 1kHz, 10kHz, 100kHz, 1MHz.
// For low frequencies (<=10Hz) use fewer ticks to keep total time reasonable.

static void run_frequency_sweep() {
    printf("\n=== Frequency Sweep ===\n");

    struct FreqSpec {
        double freq;
        int ticks;
        const char *label;
    };

    FreqSpec specs[] = {
        {1.0, 3, "1Hz"},
        {10.0, 10, "10Hz"},
        {100.0, 50, "100Hz"},
        {1000.0, 100, "1kHz"},
        {10000.0, 1000, "10kHz"},
        {100000.0, 5000, "100kHz"},
        {1000000.0, 10000, "1MHz"},
    };

    for (auto &spec : specs) {
        Timer timer(spec.freq);
        std::vector<int64_t> latencies;
        latencies.reserve(spec.ticks);
        int overruns = 0;

        for (int i = 0; i < spec.ticks; ++i) {
            timer.wait();
            if (timer.overrun()) {
                ++overruns;
            }
            latencies.push_back(timer.last_latency().count());
        }

        auto stats = compute_stats(latencies, overruns);
        print_stats(spec.label, spec.freq, stats);
    }
}

// ── Jitter histogram ────────────────────────────────────────────────────────
//
// 10,000 ticks at 10kHz, collect latency distribution.

static void run_jitter_histogram() {
    printf("\n=== Jitter Histogram (10kHz, 10000 ticks) ===\n");

    const double freq = 10000.0;
    const int n_ticks = 10000;

    Timer timer(freq);
    std::vector<int64_t> latencies;
    latencies.reserve(n_ticks);
    int overruns = 0;

    for (int i = 0; i < n_ticks; ++i) {
        timer.wait();
        if (timer.overrun()) {
            ++overruns;
        }
        latencies.push_back(timer.last_latency().count());
    }

    auto stats = compute_stats(latencies, overruns);
    print_stats("jitter_10kHz", freq, stats);

    // Print histogram buckets (log scale)
    printf("[timer_bench] histogram buckets (ns):\n");
    int buckets[] = {100, 1000, 10000, 100000, 1000000, 10000000};
    int prev = 0;
    for (int b : buckets) {
        int count = 0;
        for (auto v : latencies) {
            if (v >= prev && v < b)
                ++count;
        }
        if (count > 0) {
            printf("[timer_bench]   [%8d, %8d) ns: %d (%.1f%%)\n", prev, b, count,
                   100.0 * count / n_ticks);
        }
        prev = b;
    }
    // Overflow bucket
    int overflow = 0;
    for (auto v : latencies) {
        if (v >= buckets[5])
            ++overflow;
    }
    if (overflow > 0) {
        printf("[timer_bench]   [%8d,      inf) ns: %d (%.1f%%)\n", buckets[5], overflow,
               100.0 * overflow / n_ticks);
    }
}

// ── Overrun recovery ────────────────────────────────────────────────────────
//
// Force overruns via sleep, measure recovery time after reset_phase().

static void run_overrun_recovery() {
    printf("\n=== Overrun Recovery ===\n");

    const double freq = 1000.0; // 1kHz
    Timer timer(freq);

    // Warm up with 10 normal ticks
    for (int i = 0; i < 10; ++i) {
        timer.wait();
    }

    // Force an overrun by sleeping through several ticks
    std::this_thread::sleep_for(std::chrono::milliseconds(50)); // Miss ~50 ticks at 1kHz
    timer.wait();
    int64_t missed = timer.missed_count();
    bool was_overrun = timer.overrun();
    int64_t overrun_latency = timer.last_latency().count();

    printf("[timer_bench] overrun: detected=%s missed_count=%ld latency=%ld ns\n",
           was_overrun ? "yes" : "no", missed, overrun_latency);

    // Recovery via reset_phase()
    timer.reset_phase();

    // Measure post-recovery latency
    std::vector<int64_t> recovery_latencies;
    recovery_latencies.reserve(100);
    int post_overruns = 0;
    for (int i = 0; i < 100; ++i) {
        timer.wait();
        if (timer.overrun())
            ++post_overruns;
        recovery_latencies.push_back(timer.last_latency().count());
    }

    auto stats = compute_stats(recovery_latencies, post_overruns);
    print_stats("post_recovery", freq, stats);
}

// ── Wake-up latency ─────────────────────────────────────────────────────────
//
// 1000 ticks at 1kHz, report best/worst/median wake-up deviation.

static void run_wakeup_latency() {
    printf("\n=== Wake-up Latency (1kHz, 1000 ticks) ===\n");

    const double freq = 1000.0;
    const int n_ticks = 1000;

    Timer timer(freq);
    std::vector<int64_t> latencies;
    latencies.reserve(n_ticks);
    int overruns = 0;

    for (int i = 0; i < n_ticks; ++i) {
        timer.wait();
        if (timer.overrun())
            ++overruns;
        latencies.push_back(timer.last_latency().count());
    }

    auto stats = compute_stats(latencies, overruns);
    print_stats("wakeup_1kHz", freq, stats);
}

// ── Main ────────────────────────────────────────────────────────────────────

int main() {
    printf("=== Pipit Timer Precision Benchmarks ===\n");
    printf("Clock: steady_clock\n");

    run_frequency_sweep();
    run_jitter_histogram();
    run_overrun_recovery();
    run_wakeup_latency();

    printf("\n=== Done ===\n");
    return 0;
}
