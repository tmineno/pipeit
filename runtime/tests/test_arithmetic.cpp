//
// test_arithmetic.cpp — Runtime tests for arithmetic actors
//
// Tests basic arithmetic operations: add, sub, mul, div, abs, sqrt, threshold
//

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <pipit.h>
#include <std_math.h>

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

// ── Test: mul actor ──
TEST(mul_basic) {
    Actor_mul<float> actor{2.0f};
    actor.N = 1;
    float in[1] = {5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 10.0f);
}

TEST(mul_negative_gain) {
    Actor_mul<float> actor{-2.0f};
    actor.N = 1;
    float in[1] = {5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], -10.0f);
}

// ── Test: add actor ──
TEST(add_basic) {
    Actor_add<float> actor;
    float in[2] = {3.0f, 7.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 10.0f);
}

TEST(add_negative) {
    Actor_add<float> actor;
    float in[2] = {-3.0f, 7.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 4.0f);
}

// ── Test: sub actor ──
TEST(sub_basic) {
    Actor_sub<float> actor;
    float in[2] = {10.0f, 3.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 7.0f);
}

TEST(sub_negative_result) {
    Actor_sub<float> actor;
    float in[2] = {3.0f, 10.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], -7.0f);
}

// ── Test: div actor ──
TEST(div_basic) {
    Actor_div<float> actor;
    float in[2] = {10.0f, 2.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 5.0f);
}

TEST(div_by_zero_returns_nan) {
    Actor_div<float> actor;
    float in[2] = {10.0f, 0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_TRUE(std::isnan(out[0]));
}

// ── Test: abs actor ──
TEST(abs_positive) {
    Actor_abs<float> actor;
    float in[1] = {5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 5.0f);
}

TEST(abs_negative) {
    Actor_abs<float> actor;
    float in[1] = {-5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 5.0f);
}

TEST(abs_zero) {
    Actor_abs<float> actor;
    float in[1] = {0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0.0f);
}

// ── Test: sqrt actor ──
TEST(sqrt_basic) {
    Actor_sqrt<float> actor;
    float in[1] = {16.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 4.0f);
}

TEST(sqrt_zero) {
    Actor_sqrt<float> actor;
    float in[1] = {0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0.0f);
}

TEST(sqrt_negative_returns_nan) {
    Actor_sqrt<float> actor;
    float in[1] = {-4.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_TRUE(std::isnan(out[0]));
}

// ── Test: threshold actor ──
TEST(threshold_above) {
    Actor_threshold<float> actor{0.5f};
    float in[1] = {0.7f};
    int32_t out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 1);
}

TEST(threshold_below) {
    Actor_threshold<float> actor{0.5f};
    float in[1] = {0.3f};
    int32_t out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0);
}

TEST(threshold_exact) {
    Actor_threshold<float> actor{0.5f};
    float in[1] = {0.5f};
    int32_t out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0); // Equal to threshold is NOT greater
}

// ── Test: convolve actor ──

TEST(convolve_identity_kernel) {
    // Convolution with [1] should be identity
    float kernel[] = {1.0f};
    Actor_convolve<float> actor;
    actor.kernel = std::span<const float>(kernel, 1);
    actor.N = 4;

    float in[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    float out[4];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 1.0f, 0.0001f);
    ASSERT_NEAR(out[1], 2.0f, 0.0001f);
    ASSERT_NEAR(out[2], 3.0f, 0.0001f);
    ASSERT_NEAR(out[3], 4.0f, 0.0001f);
}

TEST(convolve_impulse_response) {
    // Convolution of impulse [1,0,0,0] with kernel [a,b,c] should yield [a,b,c,0]
    float kernel[] = {0.5f, 0.3f, 0.2f};
    Actor_convolve<float> actor;
    actor.kernel = std::span<const float>(kernel, 3);
    actor.N = 4;

    float in[4] = {1.0f, 0.0f, 0.0f, 0.0f};
    float out[4];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 0.5f, 0.0001f);
    ASSERT_NEAR(out[1], 0.3f, 0.0001f);
    ASSERT_NEAR(out[2], 0.2f, 0.0001f);
    ASSERT_NEAR(out[3], 0.0f, 0.0001f);
}

TEST(convolve_smoothing) {
    // Moving average kernel [0.25, 0.5, 0.25] on a step function
    float kernel[] = {0.25f, 0.5f, 0.25f};
    Actor_convolve<float> actor;
    actor.kernel = std::span<const float>(kernel, 3);
    actor.N = 6;

    float in[6] = {0.0f, 0.0f, 0.0f, 1.0f, 1.0f, 1.0f};
    float out[6];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    // out[0] = 0.25*0 = 0
    ASSERT_NEAR(out[0], 0.0f, 0.0001f);
    // out[3] = 0.25*1 + 0.5*0 + 0.25*0 = 0.25
    ASSERT_NEAR(out[3], 0.25f, 0.0001f);
    // out[4] = 0.25*1 + 0.5*1 + 0.25*0 = 0.75
    ASSERT_NEAR(out[4], 0.75f, 0.0001f);
    // out[5] = 0.25*1 + 0.5*1 + 0.25*1 = 1.0
    ASSERT_NEAR(out[5], 1.0f, 0.0001f);
}

TEST(convolve_linearity) {
    // convolve(a*x, k) == a * convolve(x, k)
    float kernel[] = {0.3f, 0.5f, 0.2f};
    Actor_convolve<float> actor1, actor2;
    actor1.kernel = std::span<const float>(kernel, 3);
    actor1.N = 4;
    actor2.kernel = std::span<const float>(kernel, 3);
    actor2.N = 4;

    float x[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    float scaled[4];
    float out_x[4], out_scaled[4];
    const float a = 2.5f;

    for (int i = 0; i < 4; ++i)
        scaled[i] = a * x[i];

    actor1(x, out_x);
    actor2(scaled, out_scaled);

    for (int i = 0; i < 4; ++i) {
        ASSERT_NEAR(out_scaled[i], a * out_x[i], 0.0001f);
    }
}

int main() {
    printf("\n=== Arithmetic Actor Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
