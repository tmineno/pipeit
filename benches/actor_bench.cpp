// Actor microbenchmarks — per-actor firing cost
//
// Measures the raw compute cost of individual actor firings,
// isolated from timer/buffer/scheduling overhead.

#include <benchmark/benchmark.h>
#include <cmath>
#include <complex>
#include <cstring>
#include <pipit.h>
#include <std_actors.h>

using namespace pipit;

// ── Helpers ─────────────────────────────────────────────────────────────────

// Fill buffer with deterministic test data
static void fill_float(float *buf, int n) {
    for (int i = 0; i < n; ++i)
        buf[i] = static_cast<float>(i) * 0.01f + 0.5f;
}

static void fill_cfloat(cfloat *buf, int n) {
    for (int i = 0; i < n; ++i)
        buf[i] = cfloat(static_cast<float>(i) * 0.01f, 0.0f);
}

// ── Arithmetic actors ───────────────────────────────────────────────────────

static void BM_Actor_mul(benchmark::State &state) {
    const int N = 64;
    float in[N], out[N];
    fill_float(in, N);
    Actor_mul actor{2.0f, N};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_mul);

static void BM_Actor_add(benchmark::State &state) {
    float in[2] = {1.5f, 2.5f};
    float out[1];
    Actor_add actor{};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_Actor_add);

static void BM_Actor_sub(benchmark::State &state) {
    float in[2] = {3.5f, 1.5f};
    float out[1];
    Actor_sub actor{};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_Actor_sub);

static void BM_Actor_div(benchmark::State &state) {
    float in[2] = {7.0f, 2.0f};
    float out[1];
    Actor_div actor{};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_Actor_div);

static void BM_Actor_abs(benchmark::State &state) {
    float in[1] = {-3.14f};
    float out[1];
    Actor_abs actor{};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_Actor_abs);

static void BM_Actor_sqrt(benchmark::State &state) {
    float in[1] = {16.0f};
    float out[1];
    Actor_sqrt actor{};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations());
}
BENCHMARK(BM_Actor_sqrt);

// ── FFT (parametrized by N) ─────────────────────────────────────────────────

static void BM_Actor_fft(benchmark::State &state) {
    const int N = static_cast<int>(state.range(0));
    std::vector<float> in(N);
    std::vector<cfloat> out(N);
    fill_float(in.data(), N);
    Actor_fft actor{N};

    for (auto _ : state) {
        actor(in.data(), out.data());
        benchmark::DoNotOptimize(out.data());
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_fft)->Arg(64)->Arg(256)->Arg(1024)->Arg(4096);

// ── FIR (parametrized by tap count) ─────────────────────────────────────────

static void BM_Actor_fir_5tap(benchmark::State &state) {
    const int N = 5;
    float coeff_data[] = {0.1f, 0.2f, 0.4f, 0.2f, 0.1f};
    float in[N], out[1];
    fill_float(in, N);
    Actor_fir actor{N, std::span<const float>(coeff_data, N)};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_fir_5tap);

static void BM_Actor_fir_16tap(benchmark::State &state) {
    const int N = 16;
    float coeff_data[N];
    for (int i = 0; i < N; ++i)
        coeff_data[i] = 1.0f / N;
    float in[N], out[1];
    fill_float(in, N);
    Actor_fir actor{N, std::span<const float>(coeff_data, N)};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_fir_16tap);

static void BM_Actor_fir_64tap(benchmark::State &state) {
    const int N = 64;
    float coeff_data[N];
    for (int i = 0; i < N; ++i)
        coeff_data[i] = 1.0f / N;
    float in[N], out[1];
    fill_float(in, N);
    Actor_fir actor{N, std::span<const float>(coeff_data, N)};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_fir_64tap);

// ── Statistics actors ───────────────────────────────────────────────────────

static void BM_Actor_mean(benchmark::State &state) {
    const int N = 64;
    float in[N], out[1];
    fill_float(in, N);
    Actor_mean actor{N};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_mean);

static void BM_Actor_rms(benchmark::State &state) {
    const int N = 64;
    float in[N], out[1];
    fill_float(in, N);
    Actor_rms actor{N};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_rms);

static void BM_Actor_min(benchmark::State &state) {
    const int N = 64;
    float in[N], out[1];
    fill_float(in, N);
    Actor_min actor{N};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_min);

static void BM_Actor_max(benchmark::State &state) {
    const int N = 64;
    float in[N], out[1];
    fill_float(in, N);
    Actor_max actor{N};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_max);

// ── Transform actors ────────────────────────────────────────────────────────

static void BM_Actor_c2r(benchmark::State &state) {
    const int N = 256;
    cfloat in[N];
    float out[N];
    fill_cfloat(in, N);
    Actor_c2r actor{N};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_c2r);

static void BM_Actor_mag(benchmark::State &state) {
    const int N = 256;
    cfloat in[N];
    float out[N];
    fill_cfloat(in, N);
    Actor_mag actor{N};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_mag);

static void BM_Actor_decimate(benchmark::State &state) {
    const int N = 10;
    float in[N], out[1];
    fill_float(in, N);
    Actor_decimate actor{N};

    for (auto _ : state) {
        actor(in, out);
        benchmark::DoNotOptimize(out);
    }
    state.SetItemsProcessed(state.iterations() * N);
}
BENCHMARK(BM_Actor_decimate);

BENCHMARK_MAIN();
