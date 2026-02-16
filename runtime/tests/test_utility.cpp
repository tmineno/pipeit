//
// test_utility.cpp — Runtime tests for utility actors
//
// Tests: constant, delay, decimate
//

#include <cstdio>
#include <cstdlib>
#include <pipit.h>
#include <std_actors.h>
#include <thread>

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

// ── Test: constant actor ──
TEST(constant_zero) {
    Actor_constant actor{0.0f};
    actor.N = 1;
    float in[1]; // Unused for source actor
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 0.0f);
}

TEST(constant_positive) {
    Actor_constant actor{42.0f};
    actor.N = 1;
    float in[1];
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 42.0f);
}

TEST(constant_negative) {
    Actor_constant actor{-3.14f};
    actor.N = 1;
    float in[1];
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], -3.14f);
}

TEST(constant_multiple_calls) {
    Actor_constant actor{7.5f};
    actor.N = 1;
    float in[1];
    float out[1];

    // Call multiple times, should always return same value
    for (int i = 0; i < 10; ++i) {
        int result = actor(in, out);
        ASSERT_EQ(result, ACTOR_OK);
        ASSERT_EQ(out[0], 7.5f);
    }
}

// ── Test: delay actor ──
TEST(delay_passthrough) {
    Actor_delay actor;
    actor.N = 1;
    actor.init = 0.0f;

    float in[1] = {5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 5.0f); // Delay just passes through
}

TEST(delay_different_values) {
    Actor_delay actor;
    actor.N = 1;
    actor.init = 0.0f;

    float in[1] = {1.0f};
    float out[1];

    actor(in, out);
    ASSERT_EQ(out[0], 1.0f);

    in[0] = 2.0f;
    actor(in, out);
    ASSERT_EQ(out[0], 2.0f);

    in[0] = 3.0f;
    actor(in, out);
    ASSERT_EQ(out[0], 3.0f);
}

// ── Test: decimate actor ──
TEST(decimate_basic) {
    Actor_decimate actor;
    actor.N = 3;

    float in[3] = {1.0f, 2.0f, 3.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 1.0f); // Returns first value
}

TEST(decimate_large_window) {
    Actor_decimate actor;
    actor.N = 10;

    float in[10] = {9.0f, 8.0f, 7.0f, 6.0f, 5.0f, 4.0f, 3.0f, 2.0f, 1.0f, 0.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 9.0f); // Returns first value
}

TEST(decimate_single) {
    Actor_decimate actor;
    actor.N = 1;

    float in[1] = {42.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_EQ(out[0], 42.0f);
}

TEST(runtime_context_api_basic) {
    uint64_t t0 = pipit_now_ns();
    std::this_thread::sleep_for(std::chrono::milliseconds(1));
    uint64_t t1 = pipit_now_ns();
    if (t1 < t0) {
        fprintf(stderr, "FAIL: %s:%d: pipit_now_ns() is not monotonic\n", __FILE__, __LINE__);
        exit(1);
    }

    pipit::detail::set_actor_iteration_index(123);
    pipit::detail::set_actor_task_rate_hz(48000.0);
    ASSERT_EQ(pipit_iteration_index(), 123);
    ASSERT_EQ(pipit_task_rate_hz(), 48000.0);
}

int main() {
    printf("\n=== Utility Actor Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
