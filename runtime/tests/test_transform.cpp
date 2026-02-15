//
// test_transform.cpp — Runtime tests for transform actors
//
// Tests: c2r, mag, fir
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

// ── Test: c2r actor ──
TEST(c2r_magnitude) {
    Actor_c2r actor;
    cfloat in[1] = {cfloat(3.0f, 4.0f)}; // 3+4i, magnitude = 5
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 5.0f, 0.0001f);
}

TEST(c2r_real_only) {
    Actor_c2r actor;
    cfloat in[1] = {cfloat(7.0f, 0.0f)};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 7.0f, 0.0001f);
}

TEST(c2r_imaginary_only) {
    Actor_c2r actor;
    cfloat in[1] = {cfloat(0.0f, 5.0f)};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 5.0f, 0.0001f);
}

// ── Test: mag actor ──
TEST(mag_magnitude) {
    Actor_mag actor;
    cfloat in[1] = {cfloat(3.0f, 4.0f)};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 5.0f, 0.0001f);
}

TEST(mag_zero) {
    Actor_mag actor;
    cfloat in[1] = {cfloat(0.0f, 0.0f)};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 0.0f, 0.0001f);
}

// ── Test: fir actor ──
TEST(fir_basic) {
    Actor_fir actor;
    actor.N = 3;
    float coeff[3] = {0.5f, 0.25f, 0.25f};
    actor.coeff = std::span<const float>(coeff, 3);

    float in[3] = {1.0f, 2.0f, 3.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    // 0.5*1 + 0.25*2 + 0.25*3 = 0.5 + 0.5 + 0.75 = 1.75
    ASSERT_NEAR(out[0], 1.75f, 0.0001f);
}

TEST(fir_uniform_coefficients) {
    Actor_fir actor;
    actor.N = 4;
    float coeff[4] = {0.25f, 0.25f, 0.25f, 0.25f};
    actor.coeff = std::span<const float>(coeff, 4);

    float in[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    // 0.25 * (1 + 2 + 3 + 4) = 0.25 * 10 = 2.5
    ASSERT_NEAR(out[0], 2.5f, 0.0001f);
}

TEST(fir_single_coefficient) {
    Actor_fir actor;
    actor.N = 1;
    float coeff[1] = {2.0f};
    actor.coeff = std::span<const float>(coeff, 1);

    float in[1] = {5.0f};
    float out[1];

    int result = actor(in, out);
    ASSERT_EQ(result, ACTOR_OK);
    ASSERT_NEAR(out[0], 10.0f, 0.0001f);
}

int main() {
    printf("\n=== Transform Actor Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
