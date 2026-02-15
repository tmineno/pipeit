//
// test_fft.cpp — Runtime tests for FFT actor
//
// Tests Cooley-Tukey FFT implementation
//

#include <cmath>
#include <complex>
#include <cstdio>
#include <cstdlib>
#include <pipit.h>
#include <std_actors.h>

#define TEST(name)                                                                                 \
    static void test_##name();                                                                     \
    static struct TestRunner_##name {                                                              \
        TestRunner_##name() {                                                                      \
            printf("Running test: %s\n", #name);                                                   \
            test_##name();                                                                         \
            printf("  PASS: %s\n", #name);                                                         \
        }                                                                                          \
    } runner_##name;                                                                               \
    static void test_##name()

#define ASSERT_EQ(actual, expected)                                                                \
    do {                                                                                           \
        auto _a = (actual);                                                                        \
        auto _e = (expected);                                                                      \
        if (_a != _e) {                                                                            \
            fprintf(stderr, "FAIL: %s:%d: expected %d, got %d\n", __FILE__, __LINE__, (int)_e,     \
                    (int)_a);                                                                      \
            exit(1);                                                                               \
        }                                                                                          \
    } while (0)

#define ASSERT_NEAR(actual, expected, epsilon)                                                     \
    do {                                                                                           \
        auto _a = (actual);                                                                        \
        auto _e = (expected);                                                                      \
        auto _eps = (epsilon);                                                                     \
        if (std::abs(_a - _e) > _eps) {                                                            \
            fprintf(stderr, "FAIL: %s:%d: expected %f ± %f, got %f\n", __FILE__, __LINE__,         \
                    (double)_e, (double)_eps, (double)_a);                                         \
            exit(1);                                                                               \
        }                                                                                          \
    } while (0)

#define ASSERT_COMPLEX_NEAR(actual, expected, epsilon)                                             \
    do {                                                                                           \
        auto _a = (actual);                                                                        \
        auto _e = (expected);                                                                      \
        auto _eps = (epsilon);                                                                     \
        if (std::abs(_a - _e) > _eps) {                                                            \
            fprintf(stderr, "FAIL: %s:%d: expected (%f, %f) ± %f, got (%f, %f)\n", __FILE__,       \
                    __LINE__, (double)_e.real(), (double)_e.imag(), (double)_eps,                  \
                    (double)_a.real(), (double)_a.imag());                                         \
            exit(1);                                                                               \
        }                                                                                          \
    } while (0)

// ── Test: FFT power-of-2 validation ──
TEST(fft_requires_power_of_2) {
    // Test that non-power-of-2 sizes return ACTOR_ERROR
    Actor_fft actor3;
    actor3.N = 3;
    float in3[3] = {1.0f, 2.0f, 3.0f};
    cfloat out3[3];
    ASSERT_EQ(actor3(in3, out3), ACTOR_ERROR);

    Actor_fft actor5;
    actor5.N = 5;
    float in5[5] = {1.0f, 2.0f, 3.0f, 4.0f, 5.0f};
    cfloat out5[5];
    ASSERT_EQ(actor5(in5, out5), ACTOR_ERROR);
}

// ── Test: FFT of DC signal (all zeros except DC) ──
TEST(fft_dc_signal) {
    Actor_fft actor;
    actor.N = 4;
    float in[4] = {1.0f, 1.0f, 1.0f, 1.0f};
    cfloat out[4];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);

    // DC component (bin 0) should be sum of inputs = 4.0
    ASSERT_COMPLEX_NEAR(out[0], cfloat(4.0f, 0.0f), 0.0001f);

    // All other bins should be near zero for DC signal
    for (int i = 1; i < 4; ++i) {
        ASSERT_NEAR(std::abs(out[i]), 0.0f, 0.0001f);
    }
}

// ── Test: FFT of impulse (1, 0, 0, 0) ──
TEST(fft_impulse) {
    Actor_fft actor;
    actor.N = 4;
    float in[4] = {1.0f, 0.0f, 0.0f, 0.0f};
    cfloat out[4];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);

    // FFT of impulse is all ones (flat spectrum)
    for (int i = 0; i < 4; ++i) {
        ASSERT_COMPLEX_NEAR(out[i], cfloat(1.0f, 0.0f), 0.0001f);
    }
}

// ── Test: FFT of cosine wave ──
TEST(fft_cosine_wave) {
    // 8-point FFT of cosine at bin 1 (frequency = Fs/8)
    Actor_fft actor;
    actor.N = 8;
    float in[8];
    const float PI = 3.14159265358979323846f;

    // Generate cosine wave: cos(2π * k / 8) for k=0..7
    for (int k = 0; k < 8; ++k) {
        in[k] = std::cos(2.0f * PI * k / 8.0f);
    }

    cfloat out[8];
    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);

    // Cosine at bin 1 should have peaks at bins 1 and 7 (N-1)
    // Each peak should have magnitude = N/2 = 4
    ASSERT_NEAR(std::abs(out[1]), 4.0f, 0.01f);
    ASSERT_NEAR(std::abs(out[7]), 4.0f, 0.01f);

    // Other bins should be near zero
    ASSERT_NEAR(std::abs(out[0]), 0.0f, 0.01f);
    for (int i = 2; i <= 6; ++i) {
        ASSERT_NEAR(std::abs(out[i]), 0.0f, 0.01f);
    }
}

// ── Test: FFT Parseval's theorem (energy preservation) ──
TEST(fft_parsevals_theorem) {
    // Energy in time domain should equal energy in frequency domain
    Actor_fft actor;
    actor.N = 16;
    float in[16];

    // Generate random-ish signal
    for (int i = 0; i < 16; ++i) {
        in[i] = std::sin(0.5f * i) + 0.3f * std::cos(0.7f * i);
    }

    // Compute time-domain energy
    float time_energy = 0.0f;
    for (int i = 0; i < 16; ++i) {
        time_energy += in[i] * in[i];
    }

    cfloat out[16];
    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);

    // Compute frequency-domain energy
    float freq_energy = 0.0f;
    for (int i = 0; i < 16; ++i) {
        freq_energy += std::norm(out[i]);
    }

    // Parseval: sum(|x[n]|²) = (1/N) * sum(|X[k]|²)
    ASSERT_NEAR(freq_energy / 16.0f, time_energy, 0.01f);
}

// ── Test: FFT linearity ──
TEST(fft_linearity) {
    // FFT(a*x + b*y) = a*FFT(x) + b*FFT(y)
    Actor_fft actor1, actor2, actor3;
    actor1.N = actor2.N = actor3.N = 8;

    float x[8] = {1.0f, 2.0f, 3.0f, 4.0f, 5.0f, 6.0f, 7.0f, 8.0f};
    float y[8] = {8.0f, 7.0f, 6.0f, 5.0f, 4.0f, 3.0f, 2.0f, 1.0f};
    float combined[8];

    const float a = 2.0f;
    const float b = 3.0f;

    // Compute a*x + b*y
    for (int i = 0; i < 8; ++i) {
        combined[i] = a * x[i] + b * y[i];
    }

    cfloat fft_x[8], fft_y[8], fft_combined[8];

    actor1(x, fft_x);
    actor2(y, fft_y);
    actor3(combined, fft_combined);

    // Verify FFT(a*x + b*y) = a*FFT(x) + b*FFT(y)
    for (int i = 0; i < 8; ++i) {
        cfloat expected = a * fft_x[i] + b * fft_y[i];
        ASSERT_COMPLEX_NEAR(fft_combined[i], expected, 0.001f);
    }
}

// ── Test: FFT size 256 (realistic size) ──
TEST(fft_size_256) {
    Actor_fft actor;
    actor.N = 256;
    float in[256];

    // Generate simple signal
    for (int i = 0; i < 256; ++i) {
        in[i] = (float)i / 256.0f;
    }

    cfloat out[256];
    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);

    // Just verify it completes without crashing
    // and produces non-trivial output
    float total_mag = 0.0f;
    for (int i = 0; i < 256; ++i) {
        total_mag += std::abs(out[i]);
    }

    // Should have significant magnitude
    ASSERT_NEAR(total_mag > 100.0f, true, 0.0f);
}

int main() {
    printf("\n=== FFT Actor Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
