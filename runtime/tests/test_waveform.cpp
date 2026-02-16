//
// test_waveform.cpp — Runtime tests for waveform generator actors
//
// Tests: sine, square, sawtooth, triangle, noise, impulse
//

#include <cmath>
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
            fprintf(stderr, "FAIL: %s:%d: expected %f, got %f\n", __FILE__, __LINE__, (double)_e,  \
                    (double)_a);                                                                   \
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

#define ASSERT_TRUE(condition)                                                                     \
    do {                                                                                           \
        if (!(condition)) {                                                                        \
            fprintf(stderr, "FAIL: %s:%d: condition failed: %s\n", __FILE__, __LINE__,             \
                    #condition);                                                                   \
            exit(1);                                                                               \
        }                                                                                          \
    } while (0)

// ── Sine wave tests ──

TEST(sine_basic) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_sine actor;
    actor.freq = 100.0f; // 100 Hz → period = 10 samples at 1kHz
    actor.amp = 1.0f;
    actor.N = 1;

    float in[1];
    float out[1];

    // At t=0: sin(0) = 0
    int rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);
    ASSERT_NEAR(out[0], 0.0f, 1e-6f);

    // At sample 2.5 (quarter period for 100Hz at 1kHz): sin(π/2) = 1.0
    // But we can only set integer iteration index, so check quarter period
    // 100Hz at 1kHz = period of 10 samples, quarter = sample 2.5
    // Use sample 5 (half period): sin(π) ≈ 0
    pipit::detail::set_actor_iteration_index(5);
    rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);
    ASSERT_NEAR(out[0], 0.0f, 1e-6f);
}

TEST(sine_full_cycle) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_sine actor;
    actor.freq = 10.0f; // 10 Hz → period = 100 samples at 1kHz
    actor.amp = 2.0f;
    actor.N = 100;

    float in[1];
    float out[100];

    int rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);

    // Verify against reference sin() for each sample
    for (int i = 0; i < 100; i++) {
        double t = static_cast<double>(i) / 1000.0;
        float expected = 2.0f * static_cast<float>(std::sin(2.0 * M_PI * 10.0 * t));
        ASSERT_NEAR(out[i], expected, 1e-5f);
    }
}

TEST(sine_amplitude) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(4000.0);

    Actor_sine actor;
    actor.freq = 1000.0f; // 1kHz at 4kHz → period = 4 samples
    actor.amp = 3.5f;
    actor.N = 1;

    float in[1];
    float out[1];

    // At sample 1 (quarter period): sin(π/2) = 1.0 → amp * 1.0 = 3.5
    pipit::detail::set_actor_iteration_index(1);
    actor(in, out);
    ASSERT_NEAR(out[0], 3.5f, 1e-5f);
}

// ── Square wave tests ──

TEST(square_basic) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_square actor;
    actor.freq = 100.0f; // period = 10 samples
    actor.amp = 1.0f;
    actor.N = 10;

    float in[1];
    float out[10];

    int rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);

    // First half (samples 0-4): +amp
    for (int i = 0; i < 5; i++) {
        ASSERT_EQ(out[i], 1.0f);
    }
    // Second half (samples 5-9): -amp
    for (int i = 5; i < 10; i++) {
        ASSERT_EQ(out[i], -1.0f);
    }
}

TEST(square_duty_cycle) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_square actor;
    actor.freq = 100.0f;
    actor.amp = 2.0f;
    actor.N = 20; // Two full periods

    float in[1];
    float out[20];

    actor(in, out);

    // Both periods should be identical
    for (int i = 0; i < 10; i++) {
        ASSERT_EQ(out[i], out[i + 10]);
    }
}

// ── Sawtooth wave tests ──

TEST(sawtooth_basic) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_sawtooth actor;
    actor.freq = 100.0f; // period = 10 samples
    actor.amp = 1.0f;
    actor.N = 10;

    float in[1];
    float out[10];

    int rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);

    // Sawtooth ramps from -amp to +amp over one period
    // phase at sample i = i/10, value = amp * (2*phase - 1)
    for (int i = 0; i < 10; i++) {
        double phase = static_cast<double>(i) / 10.0;
        float expected = static_cast<float>(2.0 * phase - 1.0);
        ASSERT_NEAR(out[i], expected, 1e-6f);
    }
}

// ── Triangle wave tests ──

TEST(triangle_basic) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_triangle actor;
    actor.freq = 100.0f; // period = 10 samples
    actor.amp = 1.0f;
    actor.N = 10;

    float in[1];
    float out[10];

    int rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);

    // Triangle: 4*|phase-0.5| - 1
    // phase=0.0 → -1, phase=0.25 → 0, phase=0.5 → +1, phase=0.75 → 0
    for (int i = 0; i < 10; i++) {
        double phase = static_cast<double>(i) / 10.0;
        float expected = static_cast<float>(4.0 * std::abs(phase - 0.5) - 1.0);
        ASSERT_NEAR(out[i], expected, 1e-6f);
    }
}

TEST(triangle_symmetry) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(2000.0);

    Actor_triangle actor;
    actor.freq = 100.0f; // period = 20 samples
    actor.amp = 1.0f;
    actor.N = 20;

    float in[1];
    float out[20];

    actor(in, out);

    // At sample 0: phase=0 → 4*|0-0.5|-1 = +1 (maximum)
    ASSERT_NEAR(out[0], 1.0f, 1e-6f);
    // At sample 10: phase=0.5 → 4*|0.5-0.5|-1 = -1 (minimum)
    ASSERT_NEAR(out[10], -1.0f, 1e-6f);
}

// ── Noise tests ──

TEST(noise_range) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_noise actor;
    actor.amp = 0.5f;
    actor.N = 1000;

    float in[1];
    float out[1000];

    int rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);

    // All samples should be in [-amp, +amp]
    for (int i = 0; i < 1000; i++) {
        ASSERT_TRUE(out[i] >= -0.5f && out[i] <= 0.5f);
    }
}

TEST(noise_not_constant) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_noise actor;
    actor.amp = 1.0f;
    actor.N = 100;

    float in[1];
    float out[100];

    actor(in, out);

    // At least some samples should differ from each other
    bool all_same = true;
    for (int i = 1; i < 100; i++) {
        if (out[i] != out[0]) {
            all_same = false;
            break;
        }
    }
    ASSERT_TRUE(!all_same);
}

// ── Impulse tests ──

TEST(impulse_basic) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_impulse actor;
    actor.period = 10;
    actor.N = 10;

    float in[1];
    float out[10];

    int rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);

    // Sample 0 should be 1.0 (impulse)
    ASSERT_EQ(out[0], 1.0f);

    // Samples 1-9 should be 0.0
    for (int i = 1; i < 10; i++) {
        ASSERT_EQ(out[i], 0.0f);
    }
}

TEST(impulse_periodic) {
    pipit::detail::set_actor_iteration_index(0);
    pipit::detail::set_actor_task_rate_hz(1000.0);

    Actor_impulse actor;
    actor.period = 5;
    actor.N = 20;

    float in[1];
    float out[20];

    int rc = actor(in, out);
    ASSERT_EQ(rc, ACTOR_OK);

    // Impulses at samples 0, 5, 10, 15
    for (int i = 0; i < 20; i++) {
        if (i % 5 == 0) {
            ASSERT_EQ(out[i], 1.0f);
        } else {
            ASSERT_EQ(out[i], 0.0f);
        }
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

int main() {
    printf("\n=== Waveform Generator Actor Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
