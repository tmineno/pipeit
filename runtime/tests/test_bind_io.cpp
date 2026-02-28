//
// test_bind_io.cpp — Unit tests for pipit_bind_io.h (BindIoAdapter + extract_address)
//

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>

#include <pipit_bind_io.h>

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

#define ASSERT_EQ_STR(actual, expected)                                                            \
    do {                                                                                           \
        std::string _a = (actual);                                                                 \
        std::string _e = (expected);                                                               \
        if (_a != _e) {                                                                            \
            fprintf(stderr, "FAIL: %s:%d: expected '%s', got '%s'\n", __FILE__, __LINE__,          \
                    _e.c_str(), _a.c_str());                                                       \
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

// ── extract_address tests ───────────────────────────────────────────────────

TEST(extract_address_spec) {
    ASSERT_EQ_STR(pipit::extract_address("udp(\"127.0.0.1:9100\", chan=10)"), "127.0.0.1:9100");
}

TEST(extract_address_raw) {
    ASSERT_EQ_STR(pipit::extract_address("127.0.0.1:9100"), "127.0.0.1:9100");
}

TEST(extract_address_empty) { ASSERT_EQ_STR(pipit::extract_address(""), ""); }

TEST(extract_address_unix_spec) {
    ASSERT_EQ_STR(pipit::extract_address("unix_dgram(\"/tmp/sock\")"), "/tmp/sock");
}

TEST(extract_address_no_quotes) { ASSERT_EQ_STR(pipit::extract_address("raw_addr"), "raw_addr"); }

// ── Adapter no-op tests (empty endpoint) ────────────────────────────────────

TEST(adapter_out_send_no_endpoint) {
    pipit::BindState state;
    state.current_endpoint = "";
    pipit::BindIoAdapter adapter("test_out", true, pipit::net::DTYPE_F32, 0, 48000.0, "udp",
                                 &state);
    float data[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    // Should not crash — empty endpoint is no-op
    adapter.send(data, 4);
}

TEST(adapter_in_recv_no_endpoint) {
    pipit::BindState state;
    state.current_endpoint = "";
    pipit::BindIoAdapter adapter("test_in", false, pipit::net::DTYPE_F32, 0, 48000.0, "udp",
                                 &state);
    float data[4] = {99.0f, 99.0f, 99.0f, 99.0f};
    // Should zero-fill
    adapter.recv(data, 4);
    for (int i = 0; i < 4; ++i) {
        ASSERT_TRUE(data[i] == 0.0f);
    }
}

// ── Reconnect tests ─────────────────────────────────────────────────────────

TEST(adapter_reconnect_empty) {
    pipit::BindState state;
    state.current_endpoint = "127.0.0.1:19100";
    pipit::BindIoAdapter adapter("test_reconnect", true, pipit::net::DTYPE_F32, 0, 48000.0, "udp",
                                 &state);
    // Reconnect to empty — should not crash, subsequent I/O is no-op
    adapter.reconnect("");
    float data[1] = {1.0f};
    adapter.send(data, 1);
}

// ── Loopback send/recv test ─────────────────────────────────────────────────

TEST(adapter_loopback_send_recv) {
    // Use a loopback UDP port for testing
    const char *addr = "127.0.0.1:19200";
    std::string spec = std::string("udp(\"") + addr + "\")";

    // Create receiver FIRST so it binds the port before sender transmits
    pipit::BindState in_state;
    in_state.current_endpoint = spec;
    pipit::BindIoAdapter in_adapter("in", false, pipit::net::DTYPE_I32, 0, 1000.0, "udp",
                                    &in_state);
    // Trigger lazy_init (binds socket) via a dummy recv
    int32_t dummy[1] = {0};
    in_adapter.recv(dummy, 1);

    pipit::BindState out_state;
    out_state.current_endpoint = spec;
    pipit::BindIoAdapter out_adapter("out", true, pipit::net::DTYPE_I32, 0, 1000.0, "udp",
                                     &out_state);

    // Send data
    int32_t send_data[2] = {42, 99};
    out_adapter.send(send_data, 2);

    // Give network a moment
    std::this_thread::sleep_for(std::chrono::milliseconds(10));

    // Receive data
    int32_t recv_data[2] = {0, 0};
    in_adapter.recv(recv_data, 2);
    ASSERT_TRUE(recv_data[0] == 42);
    ASSERT_TRUE(recv_data[1] == 99);
}

// ── recv zero-fill when no data available ───────────────────────────────────

TEST(adapter_recv_zero_fill) {
    pipit::BindState state;
    state.current_endpoint = "127.0.0.1:19201";
    pipit::BindIoAdapter adapter("zero_fill", false, pipit::net::DTYPE_F32, 0, 1000.0, "udp",
                                 &state);
    float data[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    adapter.recv(data, 4);
    for (int i = 0; i < 4; ++i) {
        ASSERT_TRUE(data[i] == 0.0f);
    }
}

// ── Concurrent send + reconnect ─────────────────────────────────────────────

TEST(adapter_concurrent_safety) {
    pipit::BindState state;
    state.current_endpoint = "";
    pipit::BindIoAdapter adapter("concurrent", true, pipit::net::DTYPE_F32, 0, 1000.0, "udp",
                                 &state);

    std::atomic<bool> done{false};
    std::thread sender([&]() {
        float data[1] = {1.0f};
        while (!done.load(std::memory_order_acquire)) {
            adapter.send(data, 1);
        }
    });

    // Reconnect from main thread while sender is active
    for (int i = 0; i < 100; ++i) {
        adapter.reconnect("");
        adapter.reconnect("127.0.0.1:19202");
    }
    done.store(true, std::memory_order_release);
    sender.join();
}

int main() {
    printf("All bind_io tests passed.\n");
    return 0;
}
