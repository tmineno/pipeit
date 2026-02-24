//
// test_shell.cpp — Unit tests for pipit_shell.h orchestration library
//
// Tests CLI parsing, probe validation, duration handling, and stats flag
// using shell_main() with mock descriptors and --duration 0 for instant exit.
//

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <pipit_shell.h>

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

#define ASSERT_TRUE(cond)                                                                          \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            fprintf(stderr, "FAIL: %s:%d: condition false: %s\n", __FILE__, __LINE__, #cond);      \
            exit(1);                                                                               \
        }                                                                                          \
    } while (0)

// ── Mock infrastructure ─────────────────────────────────────────────────────

// Shared state for mock tasks
static std::atomic<bool> g_stop{false};
static std::atomic<int> g_exit_code{0};
static std::atomic<bool> g_start{false};
static bool g_stats = false;
static FILE *g_probe_output = nullptr;

static void reset_state() {
    g_stop.store(false);
    g_exit_code.store(0);
    g_start.store(false);
    g_stats = false;
    g_probe_output = nullptr;
}

// Minimal task function: waits for start, then returns immediately
static void mock_task() {
    while (!g_start.load(std::memory_order_acquire)) {
        std::this_thread::yield();
    }
    // Exit immediately — stop flag is set by duration=0
}

static pipit::TaskStats g_task_stats{};

static pipit::ProgramDesc make_empty_desc() {
    pipit::ProgramDesc desc{};
    desc.state = {&g_stop, &g_exit_code, &g_start, &g_stats, &g_probe_output};
    desc.params = std::span<const pipit::ParamDesc>{};
    desc.tasks = std::span<const pipit::TaskDesc>{};
    desc.buffers = std::span<const pipit::BufferStatsDesc>{};
    desc.probes = std::span<const pipit::ProbeDesc>{};
    desc.overrun_policy = "drop";
    desc.mem_allocated = 1024;
    desc.mem_used = 0;
    return desc;
}

// Helper: build argv from string literals
template <size_t N> static int call_shell(const char *(&args)[N], pipit::ProgramDesc &desc) {
    return pipit::shell_main(static_cast<int>(N), const_cast<char **>(args), desc);
}

// ── Tests ───────────────────────────────────────────────────────────────────

TEST(shell_finite_duration) {
    reset_state();
    static const pipit::TaskDesc tasks[] = {{"mock", mock_task, &g_task_stats}};
    auto desc = make_empty_desc();
    desc.tasks = tasks;

    const char *args[] = {"prog", "--duration", "0"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 0);
}

TEST(shell_duration_invalid) {
    reset_state();
    auto desc = make_empty_desc();

    const char *args[] = {"prog", "--duration", "xyz"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 2);
}

TEST(shell_unknown_option) {
    reset_state();
    auto desc = make_empty_desc();

    const char *args[] = {"prog", "--bogus"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 2);
}

TEST(shell_unknown_param) {
    reset_state();
    auto desc = make_empty_desc();

    const char *args[] = {"prog", "--param", "nosuch=42"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 2);
}

TEST(shell_stats_flag) {
    reset_state();
    static const pipit::TaskDesc tasks[] = {{"mock", mock_task, &g_task_stats}};
    auto desc = make_empty_desc();
    desc.tasks = tasks;

    const char *args[] = {"prog", "--stats", "--duration", "0"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 0);
    ASSERT_TRUE(g_stats);
}

TEST(shell_probe_known) {
    reset_state();
    static bool probe_enabled = false;
    static const pipit::ProbeDesc probes[] = {{"sig", &probe_enabled}};
    static const pipit::TaskDesc tasks[] = {{"mock", mock_task, &g_task_stats}};
    auto desc = make_empty_desc();
    desc.probes = probes;
    desc.tasks = tasks;

    const char *args[] = {"prog", "--probe", "sig", "--duration", "0"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 0);
    ASSERT_TRUE(probe_enabled);
    probe_enabled = false;
}

TEST(shell_probe_unknown) {
    reset_state();
    static bool probe_enabled = false;
    static const pipit::ProbeDesc probes[] = {{"sig", &probe_enabled}};
    auto desc = make_empty_desc();
    desc.probes = probes;

    const char *args[] = {"prog", "--probe", "nonexistent"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 2);
}

TEST(shell_probe_output_missing_path) {
    reset_state();
    auto desc = make_empty_desc();

    const char *args[] = {"prog", "--probe-output"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 2);
}

TEST(shell_probe_output_open_failure) {
    reset_state();
    static bool probe_enabled = false;
    static const pipit::ProbeDesc probes[] = {{"sig", &probe_enabled}};
    auto desc = make_empty_desc();
    desc.probes = probes;

    const char *args[] = {"prog", "--probe", "sig", "--probe-output",
                          "/nonexistent_dir_12345/probe.out"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 2);
}

TEST(shell_probe_duplicate) {
    reset_state();
    static bool probe_enabled = false;
    static const pipit::ProbeDesc probes[] = {{"sig", &probe_enabled}};
    static const pipit::TaskDesc tasks[] = {{"mock", mock_task, &g_task_stats}};
    auto desc = make_empty_desc();
    desc.probes = probes;
    desc.tasks = tasks;

    const char *args[] = {"prog", "--probe", "sig", "--probe", "sig", "--duration", "0"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 0);
    probe_enabled = false;
}

TEST(shell_release_probe_ignored) {
    reset_state();
    // probes empty = release mode equivalent
    static const pipit::TaskDesc tasks[] = {{"mock", mock_task, &g_task_stats}};
    auto desc = make_empty_desc();
    desc.tasks = tasks;

    const char *args[] = {"prog", "--probe", "anything", "--duration", "0"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 0);
}

TEST(shell_release_probe_output_ignored) {
    reset_state();
    // probes empty = release mode; --probe-output with nonexistent path should not error
    static const pipit::TaskDesc tasks[] = {{"mock", mock_task, &g_task_stats}};
    auto desc = make_empty_desc();
    desc.tasks = tasks;

    const char *args[] = {"prog", "--probe-output", "/nonexistent_dir_12345/probe.out",
                          "--duration", "0"};
    int rc = call_shell(args, desc);
    ASSERT_EQ(rc, 0);
}

int main() {
    printf("All shell tests passed.\n");
    return 0;
}
