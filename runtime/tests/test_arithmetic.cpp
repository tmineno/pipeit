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

// ── Test: mul actor ──
TEST(mul_basic) {
    Actor_mul actor{2.0f};
    actor.N = 1;
    float in[1] = {5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 10.0f);
}

TEST(mul_negative_gain) {
    Actor_mul actor{-2.0f};
    actor.N = 1;
    float in[1] = {5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], -10.0f);
}

// ── Test: add actor ──
TEST(add_basic) {
    Actor_add actor;
    float in[2] = {3.0f, 7.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 10.0f);
}

TEST(add_negative) {
    Actor_add actor;
    float in[2] = {-3.0f, 7.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 4.0f);
}

// ── Test: sub actor ──
TEST(sub_basic) {
    Actor_sub actor;
    float in[2] = {10.0f, 3.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 7.0f);
}

TEST(sub_negative_result) {
    Actor_sub actor;
    float in[2] = {3.0f, 10.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], -7.0f);
}

// ── Test: div actor ──
TEST(div_basic) {
    Actor_div actor;
    float in[2] = {10.0f, 2.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 5.0f);
}

TEST(div_by_zero_returns_nan) {
    Actor_div actor;
    float in[2] = {10.0f, 0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_TRUE(std::isnan(out[0]));
}

// ── Test: abs actor ──
TEST(abs_positive) {
    Actor_abs actor;
    float in[1] = {5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 5.0f);
}

TEST(abs_negative) {
    Actor_abs actor;
    float in[1] = {-5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 5.0f);
}

TEST(abs_zero) {
    Actor_abs actor;
    float in[1] = {0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0.0f);
}

// ── Test: sqrt actor ──
TEST(sqrt_basic) {
    Actor_sqrt actor;
    float in[1] = {16.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 4.0f);
}

TEST(sqrt_zero) {
    Actor_sqrt actor;
    float in[1] = {0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0.0f);
}

TEST(sqrt_negative_returns_nan) {
    Actor_sqrt actor;
    float in[1] = {-4.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_TRUE(std::isnan(out[0]));
}

// ── Test: threshold actor ──
TEST(threshold_above) {
    Actor_threshold actor{0.5f};
    float in[1] = {0.7f};
    int32_t out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 1);
}

TEST(threshold_below) {
    Actor_threshold actor{0.5f};
    float in[1] = {0.3f};
    int32_t out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0);
}

TEST(threshold_exact) {
    Actor_threshold actor{0.5f};
    float in[1] = {0.5f};
    int32_t out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0); // Equal to threshold is NOT greater
}

int main() {
    printf("\n=== Arithmetic Actor Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
