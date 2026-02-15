//
// test_statistics.cpp — Runtime tests for statistics actors
//
// Tests: mean, rms, min, max
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

// ── Test: mean actor ──
TEST(mean_basic) {
    Actor_mean actor;
    actor.N = 5;
    float in[5] = {1.0f, 2.0f, 3.0f, 4.0f, 5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 3.0f); // (1+2+3+4+5)/5 = 15/5 = 3
}

TEST(mean_negative_values) {
    Actor_mean actor;
    actor.N = 4;
    float in[4] = {-2.0f, -4.0f, 2.0f, 4.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0.0f); // (-2-4+2+4)/4 = 0/4 = 0
}

TEST(mean_single_value) {
    Actor_mean actor;
    actor.N = 1;
    float in[1] = {42.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 42.0f);
}

TEST(mean_large_window) {
    Actor_mean actor;
    actor.N = 100;
    float in[100];
    for (int i = 0; i < 100; ++i) {
        in[i] = (float)i;
    }
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 49.5f, 0.001f); // (0+1+...+99)/100 = 4950/100 = 49.5
}

// ── Test: rms actor ──
TEST(rms_basic) {
    Actor_rms actor;
    actor.N = 3;
    float in[3] = {3.0f, 4.0f, 0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 2.8867513f, 0.0001f); // sqrt((9+16+0)/3) = sqrt(25/3) ≈ 2.887
}

TEST(rms_uniform_values) {
    Actor_rms actor;
    actor.N = 5;
    float in[5] = {2.0f, 2.0f, 2.0f, 2.0f, 2.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 2.0f, 0.0001f); // sqrt((4+4+4+4+4)/5) = sqrt(20/5) = 2
}

TEST(rms_zero) {
    Actor_rms actor;
    actor.N = 3;
    float in[3] = {0.0f, 0.0f, 0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0.0f);
}

// ── Test: min actor ──
TEST(min_basic) {
    Actor_min actor;
    actor.N = 5;
    float in[5] = {5.0f, 2.0f, 8.0f, 1.0f, 6.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 1.0f);
}

TEST(min_negative_values) {
    Actor_min actor;
    actor.N = 4;
    float in[4] = {-2.0f, -8.0f, 3.0f, -5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], -8.0f);
}

TEST(min_single_value) {
    Actor_min actor;
    actor.N = 1;
    float in[1] = {42.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 42.0f);
}

TEST(min_all_same) {
    Actor_min actor;
    actor.N = 4;
    float in[4] = {7.0f, 7.0f, 7.0f, 7.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 7.0f);
}

// ── Test: max actor ──
TEST(max_basic) {
    Actor_max actor;
    actor.N = 5;
    float in[5] = {5.0f, 2.0f, 8.0f, 1.0f, 6.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 8.0f);
}

TEST(max_negative_values) {
    Actor_max actor;
    actor.N = 4;
    float in[4] = {-2.0f, -8.0f, -3.0f, -5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], -2.0f);
}

TEST(max_single_value) {
    Actor_max actor;
    actor.N = 1;
    float in[1] = {42.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 42.0f);
}

TEST(max_all_same) {
    Actor_max actor;
    actor.N = 4;
    float in[4] = {7.0f, 7.0f, 7.0f, 7.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 7.0f);
}

int main() {
    printf("\n=== Statistics Actor Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
