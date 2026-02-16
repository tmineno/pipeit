//
// test_receiver.cpp — E2E tests for PpktReceiver and SampleBuffer
//
// Uses POSIX UDP loopback sockets to verify receiver behavior without GUI.
// Test macro pattern follows runtime/tests/test_net.cpp.
//

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <pipit.h>
#include <pipit_net.h>
#include <unistd.h>

#include "ppkt_receiver.h"

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

#define ASSERT_FALSE(cond) ASSERT_TRUE(!(cond))

using namespace pipit::net;

// ── Helper: send a PPKT packet to localhost:port ─────────────────────────────

static void send_ppkt(uint16_t port, uint16_t chan_id, const float *samples, uint32_t n,
                      double sample_rate_hz = 1000.0) {
    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    PpktHeader hdr = ppkt_make_header(DTYPE_F32, chan_id);
    hdr.sample_count = n;
    hdr.payload_bytes = n * sizeof(float);
    hdr.sample_rate_hz = sample_rate_hz;
    hdr.timestamp_ns = pipit_now_ns();
    hdr.iteration_index = 0;

    size_t pkt_size = sizeof(PpktHeader) + hdr.payload_bytes;
    uint8_t pkt[65536];
    std::memcpy(pkt, &hdr, sizeof(PpktHeader));
    std::memcpy(pkt + sizeof(PpktHeader), samples, hdr.payload_bytes);

    ASSERT_TRUE(tx.send(pkt, pkt_size));
}

/// Send a raw PPKT packet with arbitrary dtype payload.
static void send_ppkt_raw(uint16_t port, uint16_t chan_id, DType dtype, const void *payload,
                          uint32_t sample_count, uint32_t payload_bytes,
                          double sample_rate_hz = 1000.0) {
    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    PpktHeader hdr = ppkt_make_header(dtype, chan_id);
    hdr.sample_count = sample_count;
    hdr.payload_bytes = payload_bytes;
    hdr.sample_rate_hz = sample_rate_hz;
    hdr.timestamp_ns = pipit_now_ns();
    hdr.iteration_index = 0;

    size_t pkt_size = sizeof(PpktHeader) + payload_bytes;
    uint8_t pkt[65536];
    std::memcpy(pkt, &hdr, sizeof(PpktHeader));
    std::memcpy(pkt + sizeof(PpktHeader), payload, payload_bytes);

    ASSERT_TRUE(tx.send(pkt, pkt_size));
}

// ── SampleBuffer unit tests ──────────────────────────────────────────────────

TEST(sample_buffer_push_snapshot) {
    pipscope::SampleBuffer buf(8);

    float data[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    buf.push(data, 4);

    ASSERT_EQ(buf.count, 4);

    // Snapshot should return the 4 samples in order
    float out[4] = {};
    size_t n = buf.snapshot(out, 4);
    ASSERT_EQ(n, 4);
    ASSERT_EQ(out[0], 1.0f);
    ASSERT_EQ(out[1], 2.0f);
    ASSERT_EQ(out[2], 3.0f);
    ASSERT_EQ(out[3], 4.0f);

    // Request more than available
    float out8[8] = {};
    n = buf.snapshot(out8, 8);
    ASSERT_EQ(n, 4);
}

TEST(sample_buffer_overflow_wraps) {
    pipscope::SampleBuffer buf(4); // capacity = 4

    // Push 6 samples into a 4-capacity buffer
    float data[6] = {1.0f, 2.0f, 3.0f, 4.0f, 5.0f, 6.0f};
    buf.push(data, 6);

    ASSERT_EQ(buf.count, 4); // capped at capacity

    // Should get the last 4 samples: 3, 4, 5, 6
    float out[4] = {};
    size_t n = buf.snapshot(out, 4);
    ASSERT_EQ(n, 4);
    ASSERT_EQ(out[0], 3.0f);
    ASSERT_EQ(out[1], 4.0f);
    ASSERT_EQ(out[2], 5.0f);
    ASSERT_EQ(out[3], 6.0f);

    // Snapshot of last 2 samples
    float out2[2] = {};
    n = buf.snapshot(out2, 2);
    ASSERT_EQ(n, 2);
    ASSERT_EQ(out2[0], 5.0f);
    ASSERT_EQ(out2[1], 6.0f);
}

TEST(sample_buffer_empty_snapshot) {
    pipscope::SampleBuffer buf(8);
    float out[4] = {99.0f, 99.0f, 99.0f, 99.0f};
    size_t n = buf.snapshot(out, 4);
    ASSERT_EQ(n, 0);
}

TEST(sample_buffer_wraparound_two_pushes) {
    pipscope::SampleBuffer buf(4);

    // First push fills buffer
    float d1[3] = {1.0f, 2.0f, 3.0f};
    buf.push(d1, 3);

    // Second push wraps around
    float d2[3] = {4.0f, 5.0f, 6.0f};
    buf.push(d2, 3);

    // Should get last 4: 3, 4, 5, 6
    float out[4] = {};
    size_t n = buf.snapshot(out, 4);
    ASSERT_EQ(n, 4);
    ASSERT_EQ(out[0], 3.0f);
    ASSERT_EQ(out[1], 4.0f);
    ASSERT_EQ(out[2], 5.0f);
    ASSERT_EQ(out[3], 6.0f);
}

// ── convert_to_float tests ───────────────────────────────────────────────────

TEST(convert_f32) {
    float src[3] = {1.5f, -2.5f, 3.0f};
    float dst[3] = {};
    size_t n = pipscope::convert_to_float(reinterpret_cast<uint8_t *>(src), 3, DTYPE_F32, dst);
    ASSERT_EQ(n, 3);
    ASSERT_EQ(dst[0], 1.5f);
    ASSERT_EQ(dst[1], -2.5f);
    ASSERT_EQ(dst[2], 3.0f);
}

TEST(convert_i16) {
    int16_t src[3] = {100, -200, 32767};
    float dst[3] = {};
    size_t n = pipscope::convert_to_float(reinterpret_cast<uint8_t *>(src), 3, DTYPE_I16, dst);
    ASSERT_EQ(n, 3);
    ASSERT_EQ(dst[0], 100.0f);
    ASSERT_EQ(dst[1], -200.0f);
    ASSERT_EQ(dst[2], 32767.0f);
}

TEST(convert_i32) {
    int32_t src[2] = {1000, -1000};
    float dst[2] = {};
    size_t n = pipscope::convert_to_float(reinterpret_cast<uint8_t *>(src), 2, DTYPE_I32, dst);
    ASSERT_EQ(n, 2);
    ASSERT_EQ(dst[0], 1000.0f);
    ASSERT_EQ(dst[1], -1000.0f);
}

TEST(convert_f64) {
    double src[2] = {1.5, -2.5};
    float dst[2] = {};
    size_t n = pipscope::convert_to_float(reinterpret_cast<uint8_t *>(src), 2, DTYPE_F64, dst);
    ASSERT_EQ(n, 2);
    ASSERT_EQ(dst[0], 1.5f);
    ASSERT_EQ(dst[1], -2.5f);
}

TEST(convert_i8) {
    int8_t src[3] = {127, -128, 0};
    float dst[3] = {};
    size_t n = pipscope::convert_to_float(reinterpret_cast<uint8_t *>(src), 3, DTYPE_I8, dst);
    ASSERT_EQ(n, 3);
    ASSERT_EQ(dst[0], 127.0f);
    ASSERT_EQ(dst[1], -128.0f);
    ASSERT_EQ(dst[2], 0.0f);
}

// ── PpktReceiver E2E tests (UDP loopback) ────────────────────────────────────

TEST(receiver_starts_and_stops) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(19900));
    usleep(5000);
    rx.stop();
    // No crash = pass
}

TEST(receiver_no_data_empty_snapshot) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(19901));
    usleep(5000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 0);

    rx.stop();
}

TEST(receiver_single_channel) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(19902));
    usleep(5000);

    // Send 4 float samples to channel 0
    float samples[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    send_ppkt(19902, 0, samples, 4, 48000.0);

    usleep(10000); // wait for receiver thread to process

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].chan_id, 0);
    ASSERT_EQ(snaps[0].sample_rate_hz, 48000.0);
    ASSERT_EQ(snaps[0].packet_count, 1);
    ASSERT_EQ(snaps[0].samples.size(), 4);
    ASSERT_EQ(snaps[0].samples[0], 1.0f);
    ASSERT_EQ(snaps[0].samples[1], 2.0f);
    ASSERT_EQ(snaps[0].samples[2], 3.0f);
    ASSERT_EQ(snaps[0].samples[3], 4.0f);

    rx.stop();
}

TEST(receiver_multi_channel) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(19903));
    usleep(5000);

    // Send to channel 0
    float ch0[3] = {10.0f, 20.0f, 30.0f};
    send_ppkt(19903, 0, ch0, 3, 1000.0);

    // Send to channel 5
    float ch5[2] = {100.0f, 200.0f};
    send_ppkt(19903, 5, ch5, 2, 48000.0);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 2);

    // Channels are ordered by chan_id (std::map)
    ASSERT_EQ(snaps[0].chan_id, 0);
    ASSERT_EQ(snaps[0].samples.size(), 3);
    ASSERT_EQ(snaps[0].samples[0], 10.0f);
    ASSERT_EQ(snaps[0].sample_rate_hz, 1000.0);

    ASSERT_EQ(snaps[1].chan_id, 5);
    ASSERT_EQ(snaps[1].samples.size(), 2);
    ASSERT_EQ(snaps[1].samples[0], 100.0f);
    ASSERT_EQ(snaps[1].sample_rate_hz, 48000.0);

    rx.stop();
}

TEST(receiver_validates_ppkt) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(19904));
    usleep(5000);

    // Send a packet with invalid magic
    DatagramSender tx;
    ASSERT_TRUE(tx.open("localhost:19904", 15));

    PpktHeader hdr = ppkt_make_header(DTYPE_F32, 0);
    hdr.magic[0] = 'X'; // corrupt magic
    hdr.sample_count = 2;
    hdr.payload_bytes = 8;
    float samples[2] = {99.0f, 99.0f};

    uint8_t pkt[sizeof(PpktHeader) + 8];
    std::memcpy(pkt, &hdr, sizeof(PpktHeader));
    std::memcpy(pkt + sizeof(PpktHeader), samples, 8);
    ASSERT_TRUE(tx.send(pkt, sizeof(pkt)));

    usleep(10000);

    // Invalid packet should be ignored
    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 0);

    rx.stop();
}

TEST(receiver_multiple_packets_accumulate) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(19905));
    usleep(5000);

    // Send 3 packets to same channel
    float s1[2] = {1.0f, 2.0f};
    float s2[2] = {3.0f, 4.0f};
    float s3[2] = {5.0f, 6.0f};
    send_ppkt(19905, 0, s1, 2);
    usleep(1000);
    send_ppkt(19905, 0, s2, 2);
    usleep(1000);
    send_ppkt(19905, 0, s3, 2);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].packet_count, 3);
    ASSERT_EQ(snaps[0].samples.size(), 6); // 2+2+2 accumulated
    ASSERT_EQ(snaps[0].samples[0], 1.0f);
    ASSERT_EQ(snaps[0].samples[1], 2.0f);
    ASSERT_EQ(snaps[0].samples[2], 3.0f);
    ASSERT_EQ(snaps[0].samples[3], 4.0f);
    ASSERT_EQ(snaps[0].samples[4], 5.0f);
    ASSERT_EQ(snaps[0].samples[5], 6.0f);

    rx.stop();
}

TEST(receiver_dtype_i16_conversion) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(19906));
    usleep(5000);

    // Send i16 samples
    int16_t i16_samples[3] = {1000, -2000, 32767};
    send_ppkt_raw(19906, 0, DTYPE_I16, i16_samples, 3, 6);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 3);
    ASSERT_EQ(snaps[0].samples[0], 1000.0f);
    ASSERT_EQ(snaps[0].samples[1], -2000.0f);
    ASSERT_EQ(snaps[0].samples[2], 32767.0f);

    rx.stop();
}

TEST(receiver_dtype_f64_conversion) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(19907));
    usleep(5000);

    // Send f64 samples
    double f64_samples[2] = {3.14, -2.718};
    send_ppkt_raw(19907, 0, DTYPE_F64, f64_samples, 2, 16);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 2);
    // Compare with tolerance for float precision
    ASSERT_TRUE(snaps[0].samples[0] > 3.13f && snaps[0].samples[0] < 3.15f);
    ASSERT_TRUE(snaps[0].samples[1] > -2.72f && snaps[0].samples[1] < -2.71f);

    rx.stop();
}

// ── Main ─────────────────────────────────────────────────────────────────────

int main() {
    printf("\n=== pipscope PpktReceiver E2E Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
