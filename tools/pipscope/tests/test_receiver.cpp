//
// test_receiver.cpp — E2E tests for PpktReceiver and SampleBuffer
//
// Uses POSIX UDP loopback sockets to verify receiver behavior without GUI.
// Test macro pattern follows runtime/tests/test_net.cpp.
//

#include <atomic>
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

static uint16_t next_test_port() {
    static std::atomic<uint16_t> next{24000};
    return next.fetch_add(1);
}

// ── Helper: send a PPKT packet to localhost:port ─────────────────────────────

/// Send a single-frame PPKT packet (both FRAME_START and FRAME_END set).
static void send_ppkt(uint16_t port, uint16_t chan_id, const float *samples, uint32_t n,
                      double sample_rate_hz = 1000.0) {
    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    PpktHeader hdr = ppkt_make_header(DTYPE_F32, chan_id);
    hdr.flags = FLAG_FRAME_START | FLAG_FRAME_END;
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

/// Send a raw PPKT packet with arbitrary dtype payload (single-frame).
static void send_ppkt_raw(uint16_t port, uint16_t chan_id, DType dtype, const void *payload,
                          uint32_t sample_count, uint32_t payload_bytes,
                          double sample_rate_hz = 1000.0) {
    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    PpktHeader hdr = ppkt_make_header(dtype, chan_id);
    hdr.flags = FLAG_FRAME_START | FLAG_FRAME_END;
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

/// Send a PPKT chunk with full control over header fields.
static void send_chunk(DatagramSender &tx, uint16_t chan_id, uint8_t flags, uint32_t sequence,
                       uint64_t iteration_index, uint64_t timestamp_ns, const float *samples,
                       uint32_t n, double sample_rate_hz = 1000.0) {
    PpktHeader hdr = ppkt_make_header(DTYPE_F32, chan_id);
    hdr.flags = flags;
    hdr.sequence = sequence;
    hdr.iteration_index = iteration_index;
    hdr.timestamp_ns = timestamp_ns;
    hdr.sample_count = n;
    hdr.payload_bytes = n * sizeof(float);
    hdr.sample_rate_hz = sample_rate_hz;

    size_t pkt_size = sizeof(PpktHeader) + hdr.payload_bytes;
    uint8_t pkt[65536];
    std::memcpy(pkt, &hdr, sizeof(PpktHeader));
    std::memcpy(pkt + sizeof(PpktHeader), samples, hdr.payload_bytes);

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
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);
    rx.stop();
    // No crash = pass
}

TEST(receiver_no_data_empty_snapshot) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 0);

    rx.stop();
}

TEST(receiver_single_channel) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    // Send 4 float samples to channel 0
    float samples[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    send_ppkt(port, 0, samples, 4, 48000.0);

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
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    // Send to channel 0
    float ch0[3] = {10.0f, 20.0f, 30.0f};
    send_ppkt(port, 0, ch0, 3, 1000.0);

    // Send to channel 5
    float ch5[2] = {100.0f, 200.0f};
    send_ppkt(port, 5, ch5, 2, 48000.0);

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
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    // Send a packet with invalid magic
    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

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
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    // Send 3 packets to same channel
    float s1[2] = {1.0f, 2.0f};
    float s2[2] = {3.0f, 4.0f};
    float s3[2] = {5.0f, 6.0f};
    send_ppkt(port, 0, s1, 2);
    usleep(1000);
    send_ppkt(port, 0, s2, 2);
    usleep(1000);
    send_ppkt(port, 0, s3, 2);

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
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    // Send i16 samples
    int16_t i16_samples[3] = {1000, -2000, 32767};
    send_ppkt_raw(port, 0, DTYPE_I16, i16_samples, 3, 6);

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
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    // Send f64 samples
    double f64_samples[2] = {3.14, -2.718};
    send_ppkt_raw(port, 0, DTYPE_F64, f64_samples, 2, 16);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 2);
    // Compare with tolerance for float precision
    ASSERT_TRUE(snaps[0].samples[0] > 3.13f && snaps[0].samples[0] < 3.15f);
    ASSERT_TRUE(snaps[0].samples[1] > -2.72f && snaps[0].samples[1] < -2.71f);

    rx.stop();
}

TEST(receiver_clamps_samples_to_payload_bytes) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    // Header claims 10 i16 samples, but payload has only 2 samples (4 bytes).
    int16_t i16_samples[2] = {111, -222};
    send_ppkt_raw(port, 0, DTYPE_I16, i16_samples, 10, 4);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 2);
    ASSERT_EQ(snaps[0].samples[0], 111.0f);
    ASSERT_EQ(snaps[0].samples[1], -222.0f);

    rx.stop();
}

// ── Strict frame integrity tests ─────────────────────────────────────────────

TEST(frame_complete_single_chunk_accepted) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    float samples[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    send_ppkt(port, 0, samples, 4);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 4);
    ASSERT_EQ(snaps[0].stats.accepted_frames, 1);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 0);

    rx.stop();
}

TEST(frame_complete_multi_chunk_accepted) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    uint64_t ts = pipit_now_ns();

    // 3-chunk frame: [1,2], [3,4], [5,6]
    float c1[2] = {1.0f, 2.0f};
    float c2[2] = {3.0f, 4.0f};
    float c3[2] = {5.0f, 6.0f};

    send_chunk(tx, 0, FLAG_FRAME_START, 0, 0, ts, c1, 2);
    usleep(500);
    send_chunk(tx, 0, 0, 1, 2, ts, c2, 2);
    usleep(500);
    send_chunk(tx, 0, FLAG_FRAME_END, 2, 4, ts, c3, 2);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 6);
    ASSERT_EQ(snaps[0].samples[0], 1.0f);
    ASSERT_EQ(snaps[0].samples[5], 6.0f);
    ASSERT_EQ(snaps[0].stats.accepted_frames, 1);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 0);

    rx.stop();
}

TEST(frame_dropped_missing_middle_chunk) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    uint64_t ts = pipit_now_ns();

    // Send start chunk (seq=0), skip middle (seq=1), send end chunk (seq=2)
    float c1[2] = {1.0f, 2.0f};
    float c3[2] = {5.0f, 6.0f};

    send_chunk(tx, 0, FLAG_FRAME_START, 0, 0, ts, c1, 2);
    usleep(500);
    // Skip seq=1
    send_chunk(tx, 0, FLAG_FRAME_END, 2, 4, ts, c3, 2);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 0); // nothing committed
    ASSERT_EQ(snaps[0].stats.accepted_frames, 0);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 1);
    ASSERT_EQ(snaps[0].stats.drop_seq_gap, 1);

    rx.stop();
}

TEST(frame_dropped_missing_end) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    uint64_t ts1 = 100000;
    uint64_t ts2 = 200000;

    // First frame: start but no end
    float c1[2] = {1.0f, 2.0f};
    send_chunk(tx, 0, FLAG_FRAME_START, 0, 0, ts1, c1, 2);
    usleep(500);

    // Second frame starts → first frame dropped (boundary)
    float c2[2] = {3.0f, 4.0f};
    send_chunk(tx, 0, FLAG_FRAME_START | FLAG_FRAME_END, 1, 0, ts2, c2, 2);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    // Only second frame committed
    ASSERT_EQ(snaps[0].samples.size(), 2);
    ASSERT_EQ(snaps[0].samples[0], 3.0f);
    ASSERT_EQ(snaps[0].stats.accepted_frames, 1);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 1);
    ASSERT_EQ(snaps[0].stats.drop_boundary, 1);

    rx.stop();
}

TEST(frame_dropped_end_without_start) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    // Send end chunk without preceding start
    float c1[2] = {1.0f, 2.0f};
    send_chunk(tx, 0, FLAG_FRAME_END, 0, 0, 100000, c1, 2);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 0);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 1);
    ASSERT_EQ(snaps[0].stats.drop_boundary, 1);

    rx.stop();
}

TEST(frame_dropped_metadata_mismatch) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    uint64_t ts = 100000;

    // Start with sample_rate=1000
    float c1[2] = {1.0f, 2.0f};
    send_chunk(tx, 0, FLAG_FRAME_START, 0, 0, ts, c1, 2, 1000.0);
    usleep(500);

    // End with sample_rate=2000 → metadata mismatch
    float c2[2] = {3.0f, 4.0f};
    send_chunk(tx, 0, FLAG_FRAME_END, 1, 2, ts, c2, 2, 2000.0);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 0);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 1);
    ASSERT_EQ(snaps[0].stats.drop_meta_mismatch, 1);

    rx.stop();
}

TEST(frame_dropped_iteration_gap) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    uint64_t ts = 100000;

    // Start: 2 samples at iter=0 → next expected iter=2
    float c1[2] = {1.0f, 2.0f};
    send_chunk(tx, 0, FLAG_FRAME_START, 0, 0, ts, c1, 2);
    usleep(500);

    // End: sequence correct (1), but iteration_index=5 instead of expected 2
    float c2[2] = {3.0f, 4.0f};
    send_chunk(tx, 0, FLAG_FRAME_END, 1, 5, ts, c2, 2);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 0);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 1);
    ASSERT_EQ(snaps[0].stats.drop_iter_gap, 1);

    rx.stop();
}

TEST(frame_recovery_after_drop) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    uint64_t ts1 = 100000;
    uint64_t ts2 = 200000;

    // First frame: start chunk, then skip to next frame (triggers boundary drop)
    float bad[2] = {99.0f, 99.0f};
    send_chunk(tx, 0, FLAG_FRAME_START, 0, 0, ts1, bad, 2);
    usleep(500);

    // Second frame: clean single-chunk frame
    float good[3] = {10.0f, 20.0f, 30.0f};
    send_chunk(tx, 0, FLAG_FRAME_START | FLAG_FRAME_END, 1, 0, ts2, good, 3);

    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    // Only good frame committed
    ASSERT_EQ(snaps[0].samples.size(), 3);
    ASSERT_EQ(snaps[0].samples[0], 10.0f);
    ASSERT_EQ(snaps[0].samples[1], 20.0f);
    ASSERT_EQ(snaps[0].samples[2], 30.0f);
    ASSERT_EQ(snaps[0].stats.accepted_frames, 1);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 1);

    rx.stop();
}

// ── E2E pipeline tests (simulates socket_write → PpktReceiver) ──────────────

TEST(e2e_single_sample_firings) {
    // Simulates socket_write with N=1 (e.g. sine | socket_write at 10MHz)
    // Multiple single-sample firings should all arrive phase-continuous.
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    PpktHeader hdr = ppkt_make_header(DTYPE_F32, 0);
    hdr.sample_rate_hz = 48000.0;
    hdr.iteration_index = 0;

    constexpr int NUM_FIRINGS = 100;
    for (int i = 0; i < NUM_FIRINGS; i++) {
        hdr.flags = (i == 0) ? FLAG_FIRST_FRAME : static_cast<uint8_t>(0);
        hdr.timestamp_ns = 1000 + i;
        hdr.iteration_index = static_cast<uint64_t>(i);

        float sample = static_cast<float>(i);
        ppkt_send_chunked(tx, hdr, &sample, 1);
        usleep(200);
    }

    usleep(20000);

    auto snaps = rx.snapshot(1024);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), NUM_FIRINGS);
    ASSERT_EQ(snaps[0].stats.accepted_frames, NUM_FIRINGS);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 0);

    // Verify phase continuity: all samples in order
    for (int i = 0; i < NUM_FIRINGS; i++) {
        ASSERT_EQ(snaps[0].samples[i], static_cast<float>(i));
    }

    rx.stop();
}

TEST(e2e_multi_sample_firings) {
    // Simulates socket_write with N=4 (4 samples per firing, fits in one chunk)
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(4096);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    PpktHeader hdr = ppkt_make_header(DTYPE_F32, 0);
    hdr.sample_rate_hz = 1000.0;

    constexpr int NUM_FIRINGS = 50;
    constexpr int SAMPLES_PER_FIRING = 4;

    for (int f = 0; f < NUM_FIRINGS; f++) {
        hdr.flags = (f == 0) ? FLAG_FIRST_FRAME : static_cast<uint8_t>(0);
        hdr.timestamp_ns = 1000 + f;
        hdr.iteration_index = static_cast<uint64_t>(f * SAMPLES_PER_FIRING);

        float samples[SAMPLES_PER_FIRING];
        for (int s = 0; s < SAMPLES_PER_FIRING; s++) {
            samples[s] = static_cast<float>(f * SAMPLES_PER_FIRING + s);
        }
        ppkt_send_chunked(tx, hdr, samples, SAMPLES_PER_FIRING);
        usleep(200);
    }

    usleep(20000);

    auto snaps = rx.snapshot(4096);
    ASSERT_EQ(snaps.size(), 1);
    size_t expected_total = NUM_FIRINGS * SAMPLES_PER_FIRING;
    ASSERT_EQ(snaps[0].samples.size(), expected_total);
    ASSERT_EQ(snaps[0].stats.accepted_frames, NUM_FIRINGS);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 0);

    for (size_t i = 0; i < expected_total; i++) {
        ASSERT_EQ(snaps[0].samples[i], static_cast<float>(i));
    }

    rx.stop();
}

TEST(e2e_chunked_firings) {
    // Simulates socket_write with N=20 and tiny MTU → 3 chunks per firing
    // Verifies multi-chunk frames across multiple firings.
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(4096);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    DatagramSender tx;
    char addr[32];
    snprintf(addr, sizeof(addr), "localhost:%u", port);
    ASSERT_TRUE(tx.open(addr, strlen(addr)));

    PpktHeader hdr = ppkt_make_header(DTYPE_F32, 0);
    hdr.sample_rate_hz = 1000.0;

    constexpr int NUM_FIRINGS = 10;
    constexpr int SAMPLES_PER_FIRING = 20;
    // MTU = 48 + 32 = 80 bytes → max 8 f32 per chunk → 3 chunks per firing
    constexpr size_t TINY_MTU = sizeof(PpktHeader) + 32;

    for (int f = 0; f < NUM_FIRINGS; f++) {
        hdr.flags = (f == 0) ? FLAG_FIRST_FRAME : static_cast<uint8_t>(0);
        hdr.timestamp_ns = 1000 + f;
        hdr.iteration_index = static_cast<uint64_t>(f * SAMPLES_PER_FIRING);

        float samples[SAMPLES_PER_FIRING];
        for (int s = 0; s < SAMPLES_PER_FIRING; s++) {
            samples[s] = static_cast<float>(f * SAMPLES_PER_FIRING + s);
        }
        ppkt_send_chunked(tx, hdr, samples, SAMPLES_PER_FIRING, TINY_MTU);
        usleep(1000); // more time for multi-chunk to arrive
    }

    usleep(30000);

    auto snaps = rx.snapshot(4096);
    ASSERT_EQ(snaps.size(), 1);
    size_t expected_total = NUM_FIRINGS * SAMPLES_PER_FIRING;
    ASSERT_EQ(snaps[0].samples.size(), expected_total);
    ASSERT_EQ(snaps[0].stats.accepted_frames, NUM_FIRINGS);
    ASSERT_EQ(snaps[0].stats.dropped_frames, 0);

    for (size_t i = 0; i < expected_total; i++) {
        ASSERT_EQ(snaps[0].samples[i], static_cast<float>(i));
    }

    rx.stop();
}

// ── Address-based start and reconnect tests ──────────────────────────────────

TEST(receiver_start_with_address_string) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);

    char addr[32];
    snprintf(addr, sizeof(addr), "0.0.0.0:%u", port);
    ASSERT_TRUE(rx.start(addr));
    usleep(5000);

    float samples[2] = {42.0f, 43.0f};
    send_ppkt(port, 0, samples, 2);
    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples.size(), 2);
    ASSERT_EQ(snaps[0].samples[0], 42.0f);

    rx.stop();
}

TEST(receiver_start_with_localhost_address) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);

    char addr[32];
    snprintf(addr, sizeof(addr), "127.0.0.1:%u", port);
    ASSERT_TRUE(rx.start(addr));
    usleep(5000);

    float samples[2] = {10.0f, 20.0f};
    send_ppkt(port, 0, samples, 2);
    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples[0], 10.0f);

    rx.stop();
}

TEST(receiver_clear_channels) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_TRUE(rx.start(port));
    usleep(5000);

    float samples[2] = {1.0f, 2.0f};
    send_ppkt(port, 0, samples, 2);
    usleep(10000);

    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);

    rx.clear_channels();

    snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 0);

    rx.stop();
}

TEST(receiver_reconnect_cycle) {
    uint16_t port1 = next_test_port();
    uint16_t port2 = next_test_port();
    pipscope::PpktReceiver rx(1024);

    // Connect to port1
    ASSERT_TRUE(rx.start(port1));
    usleep(5000);
    float s1[2] = {1.0f, 2.0f};
    send_ppkt(port1, 0, s1, 2);
    usleep(10000);
    auto snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);

    // Reconnect to port2
    rx.stop();
    rx.clear_channels();
    ASSERT_TRUE(rx.start(port2));
    usleep(5000);

    // Old data should be gone
    snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 0);

    // New data on port2 works
    float s2[2] = {10.0f, 20.0f};
    send_ppkt(port2, 0, s2, 2);
    usleep(10000);
    snaps = rx.snapshot(100);
    ASSERT_EQ(snaps.size(), 1);
    ASSERT_EQ(snaps[0].samples[0], 10.0f);

    rx.stop();
}

TEST(receiver_start_invalid_address) {
    pipscope::PpktReceiver rx(1024);
    ASSERT_FALSE(rx.start("not-a-valid-address"));
    ASSERT_FALSE(rx.start(""));
}

TEST(receiver_is_running) {
    uint16_t port = next_test_port();
    pipscope::PpktReceiver rx(1024);
    ASSERT_FALSE(rx.is_running());

    ASSERT_TRUE(rx.start(port));
    ASSERT_TRUE(rx.is_running());

    rx.stop();
    ASSERT_FALSE(rx.is_running());
}

// ── Main ─────────────────────────────────────────────────────────────────────

int main() {
    printf("\n=== pipscope PpktReceiver E2E Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
