//
// test_timer.cpp — Runtime tests for Timer adaptive spin (ADR-014)
//

#include <cstdio>
#include <cstdlib>
#include <pipit.h>

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

#define ASSERT_TRUE(cond)                                                                          \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            fprintf(stderr, "FAIL: %s:%d: %s\n", __FILE__, __LINE__, #cond);                       \
            std::abort();                                                                          \
        }                                                                                          \
    } while (0)

#define ASSERT_EQ(actual, expected)                                                                \
    do {                                                                                           \
        auto _a = (actual);                                                                        \
        auto _e = (expected);                                                                      \
        if (_a != _e) {                                                                            \
            fprintf(stderr, "FAIL: %s:%d: expected %ld, got %ld\n", __FILE__, __LINE__, (long)_e,  \
                    (long)_a);                                                                     \
            std::abort();                                                                          \
        }                                                                                          \
    } while (0)

// -- Fixed spin mode --

TEST(timer_fixed_spin_not_adaptive) {
    pipit::Timer t(1000.0, true, 5000); // 1kHz, fixed 5us spin
    ASSERT_TRUE(!t.is_adaptive());
    ASSERT_EQ(t.current_spin_threshold().count(), 5000);
}

TEST(timer_fixed_spin_zero) {
    pipit::Timer t(1000.0, true, 0); // 1kHz, no spin
    ASSERT_TRUE(!t.is_adaptive());
    ASSERT_EQ(t.current_spin_threshold().count(), 0);
}

// -- Adaptive spin mode --

TEST(timer_adaptive_mode_activation) {
    pipit::Timer t(1000.0, true, -1); // sentinel -1 → adaptive
    ASSERT_TRUE(t.is_adaptive());
    // Bootstrap at 10us
    ASSERT_EQ(t.current_spin_threshold().count(), 10000);
}

TEST(timer_adaptive_spin_converges) {
    // Run at 100Hz (10ms period) for 20 ticks — enough for EWMA to move
    pipit::Timer t(100.0, true, -1);
    for (int i = 0; i < 20; ++i) {
        t.wait();
    }
    auto threshold = t.current_spin_threshold().count();
    // Should have converged within guardrail bounds [500ns, 100us]
    ASSERT_TRUE(threshold >= 500);
    ASSERT_TRUE(threshold <= 100000);
}

TEST(timer_default_spin_10us) {
    // Default constructor spin_ns=0 → no spin, not adaptive
    pipit::Timer t(1000.0);
    ASSERT_TRUE(!t.is_adaptive());
    ASSERT_EQ(t.current_spin_threshold().count(), 0);
}

int main() {
    printf("All timer tests passed.\n");
    return 0;
}
