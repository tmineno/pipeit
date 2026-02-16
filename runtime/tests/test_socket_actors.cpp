//
// test_socket_actors.cpp — Loopback integration test for socket_write / socket_read
//
// Verifies that socket_write sends PPKT packets that socket_read can receive
// and decode correctly, using UDP loopback on localhost.
//
// Note: actors use static locals for socket state (lazy init). Each actor type
// can only be initialized once per process. Tests are structured accordingly.
//

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <pipit.h>
#include <pipit_net.h>
#include <std_sink.h>
#include <std_source.h>
#include <unistd.h>

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

#define ASSERT_TRUE(cond)                                                                          \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            fprintf(stderr, "FAIL: %s:%d: %s\n", __FILE__, __LINE__, #cond);                       \
            exit(1);                                                                               \
        }                                                                                          \
    } while (0)

// ── Test: socket_write sends valid PPKT that a raw receiver can decode ──

TEST(socket_write_sends_ppkt) {
    pipit::detail::set_actor_task_rate_hz(1000.0);
    pipit::detail::set_actor_iteration_index(42);

    // Raw receiver on port 19880
    pipit::net::DatagramReceiver rx;
    ASSERT_TRUE(rx.open("localhost:19880", 15));

    // Create socket_write actor: addr="localhost:19880", chan_id=3, N=4
    const char addr[] = "localhost:19880";
    Actor_socket_write writer{
        std::span<const char>(addr, sizeof(addr) - 1),
        3, // chan_id
        4  // N
    };

    float samples[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    int rc = writer(samples, nullptr);
    ASSERT_EQ(rc, ACTOR_OK);

    usleep(2000);

    // Receive and validate PPKT header
    uint8_t buf[512];
    ssize_t n = rx.recv(buf, sizeof(buf));
    ASSERT_TRUE(n > 0);

    pipit::net::PpktHeader hdr;
    std::memcpy(&hdr, buf, sizeof(pipit::net::PpktHeader));
    ASSERT_TRUE(pipit::net::ppkt_validate(hdr));
    ASSERT_EQ(hdr.sample_count, 4);
    ASSERT_EQ(hdr.chan_id, 3);
    ASSERT_EQ(hdr.sample_rate_hz, 1000.0);
    ASSERT_EQ(hdr.iteration_index, 42);
    ASSERT_TRUE(hdr.flags & pipit::net::FLAG_FIRST_FRAME);

    // Validate payload
    float recv[4];
    std::memcpy(recv, buf + sizeof(pipit::net::PpktHeader), 16);
    ASSERT_EQ(recv[0], 1.0f);
    ASSERT_EQ(recv[1], 2.0f);
    ASSERT_EQ(recv[2], 3.0f);
    ASSERT_EQ(recv[3], 4.0f);

    // Second firing: FLAG_FIRST_FRAME should be cleared
    pipit::detail::set_actor_iteration_index(43);
    rc = writer(samples, nullptr);
    ASSERT_EQ(rc, ACTOR_OK);

    usleep(2000);
    n = rx.recv(buf, sizeof(buf));
    ASSERT_TRUE(n > 0);
    std::memcpy(&hdr, buf, sizeof(pipit::net::PpktHeader));
    ASSERT_TRUE((hdr.flags & pipit::net::FLAG_FIRST_FRAME) == 0);
    ASSERT_EQ(hdr.iteration_index, 43);
}

// ── Test: socket_read receives PPKT, outputs zeros when no data ──

TEST(socket_read_loopback) {
    const char addr[] = "localhost:19881";
    Actor_socket_read reader{
        std::span<const char>(addr, sizeof(addr) - 1),
        4 // N
    };

    // First call: initializes socket, no data → zeros
    float out[4] = {99.0f, 99.0f, 99.0f, 99.0f};
    int rc = reader(nullptr, out);
    ASSERT_EQ(rc, ACTOR_OK);
    ASSERT_EQ(out[0], 0.0f);
    ASSERT_EQ(out[1], 0.0f);
    ASSERT_EQ(out[2], 0.0f);
    ASSERT_EQ(out[3], 0.0f);

    // Send a PPKT packet to port 19881
    pipit::net::DatagramSender tx;
    ASSERT_TRUE(tx.open("localhost:19881", 15));

    pipit::net::PpktHeader hdr = pipit::net::ppkt_make_header(pipit::net::DTYPE_F32, 0);
    hdr.sample_count = 4;
    hdr.payload_bytes = 16;
    hdr.sample_rate_hz = 1000.0;
    hdr.timestamp_ns = pipit_now_ns();
    hdr.iteration_index = 0;

    float samples[4] = {10.0f, 20.0f, 30.0f, 40.0f};
    uint8_t pkt[sizeof(pipit::net::PpktHeader) + 16];
    std::memcpy(pkt, &hdr, sizeof(pipit::net::PpktHeader));
    std::memcpy(pkt + sizeof(pipit::net::PpktHeader), samples, 16);
    ASSERT_TRUE(tx.send(pkt, sizeof(pkt)));

    usleep(2000);

    // Second call: should receive the data
    float out2[4] = {0};
    rc = reader(nullptr, out2);
    ASSERT_EQ(rc, ACTOR_OK);
    ASSERT_EQ(out2[0], 10.0f);
    ASSERT_EQ(out2[1], 20.0f);
    ASSERT_EQ(out2[2], 30.0f);
    ASSERT_EQ(out2[3], 40.0f);

    // Third call: no more data → zeros again
    float out3[4] = {99.0f, 99.0f, 99.0f, 99.0f};
    rc = reader(nullptr, out3);
    ASSERT_EQ(rc, ACTOR_OK);
    ASSERT_EQ(out3[0], 0.0f);
    ASSERT_EQ(out3[1], 0.0f);
    ASSERT_EQ(out3[2], 0.0f);
    ASSERT_EQ(out3[3], 0.0f);
}

// ── Main ──

int main() {
    printf("\n=== Socket Actor Loopback Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
