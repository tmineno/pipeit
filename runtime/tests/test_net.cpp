//
// test_net.cpp — Tests for pipit_net.h (PPKT protocol + datagram transport)
//

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <pipit.h>
#include <pipit_net.h>

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

// ── PpktHeader struct tests ──

TEST(ppkt_header_size) { ASSERT_EQ(sizeof(PpktHeader), 48); }

TEST(ppkt_header_offsets) {
    PpktHeader h{};
    auto base = reinterpret_cast<const char *>(&h);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.magic) - base, 0);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.version) - base, 4);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.header_len) - base, 5);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.dtype) - base, 6);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.flags) - base, 7);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.chan_id) - base, 8);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.reserved) - base, 10);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.sequence) - base, 12);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.sample_count) - base, 16);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.payload_bytes) - base, 20);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.sample_rate_hz) - base, 24);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.timestamp_ns) - base, 32);
    ASSERT_EQ(reinterpret_cast<const char *>(&h.iteration_index) - base, 40);
}

TEST(ppkt_make_header) {
    PpktHeader h = ppkt_make_header(DTYPE_F32, 7);
    ASSERT_TRUE(std::memcmp(h.magic, PPKT_MAGIC, 4) == 0);
    ASSERT_EQ(h.version, PPKT_VERSION);
    ASSERT_EQ(h.header_len, PPKT_HEADER_LEN);
    ASSERT_EQ(h.dtype, DTYPE_F32);
    ASSERT_EQ(h.chan_id, 7);
    ASSERT_EQ(h.reserved, 0);
    ASSERT_EQ(h.sequence, 0);
    ASSERT_EQ(h.sample_count, 0);
    ASSERT_EQ(h.payload_bytes, 0);
}

TEST(ppkt_validate_ok) {
    PpktHeader h = ppkt_make_header(DTYPE_F32, 0);
    ASSERT_TRUE(ppkt_validate(h));
}

TEST(ppkt_validate_bad_magic) {
    PpktHeader h = ppkt_make_header(DTYPE_F32, 0);
    h.magic[0] = 'X';
    ASSERT_FALSE(ppkt_validate(h));
}

TEST(ppkt_validate_bad_version) {
    PpktHeader h = ppkt_make_header(DTYPE_F32, 0);
    h.version = 99;
    ASSERT_FALSE(ppkt_validate(h));
}

// ── DType size tests ──

TEST(dtype_sizes) {
    ASSERT_EQ(dtype_size(DTYPE_F32), 4);
    ASSERT_EQ(dtype_size(DTYPE_I32), 4);
    ASSERT_EQ(dtype_size(DTYPE_CF32), 8);
    ASSERT_EQ(dtype_size(DTYPE_F64), 8);
    ASSERT_EQ(dtype_size(DTYPE_I16), 2);
    ASSERT_EQ(dtype_size(DTYPE_I8), 1);
}

// ── Address parsing tests ──

TEST(parse_inet_localhost) {
    const char *addr = "localhost:9100";
    ParsedAddr pa = parse_address(addr, std::strlen(addr));
    ASSERT_TRUE(pa.kind == AddrKind::INET);
    auto *in = reinterpret_cast<struct sockaddr_in *>(&pa.storage);
    ASSERT_EQ(in->sin_family, AF_INET);
    ASSERT_EQ(ntohs(in->sin_port), 9100);
    ASSERT_EQ(ntohl(in->sin_addr.s_addr), INADDR_LOOPBACK);
}

TEST(parse_inet_ip) {
    const char *addr = "127.0.0.1:8080";
    ParsedAddr pa = parse_address(addr, std::strlen(addr));
    ASSERT_TRUE(pa.kind == AddrKind::INET);
    auto *in = reinterpret_cast<struct sockaddr_in *>(&pa.storage);
    ASSERT_EQ(ntohs(in->sin_port), 8080);
}

TEST(parse_unix) {
    const char *addr = "unix:///tmp/test.sock";
    ParsedAddr pa = parse_address(addr, std::strlen(addr));
    ASSERT_TRUE(pa.kind == AddrKind::UNIX);
    auto *un = reinterpret_cast<struct sockaddr_un *>(&pa.storage);
    ASSERT_EQ(un->sun_family, AF_UNIX);
    ASSERT_TRUE(std::strcmp(un->sun_path, "/tmp/test.sock") == 0);
}

TEST(parse_invalid_no_port) {
    const char *addr = "localhost";
    ParsedAddr pa = parse_address(addr, std::strlen(addr));
    ASSERT_TRUE(pa.kind == AddrKind::INVALID);
}

TEST(parse_invalid_bad_port) {
    const char *addr = "localhost:0";
    ParsedAddr pa = parse_address(addr, std::strlen(addr));
    ASSERT_TRUE(pa.kind == AddrKind::INVALID);
}

// ── UDP loopback send/recv test ──

TEST(udp_loopback_single_packet) {
    // Receiver: bind to a random port
    DatagramReceiver rx;
    ASSERT_TRUE(rx.open("localhost:19871", 15));

    // Sender: connect to same port
    DatagramSender tx;
    ASSERT_TRUE(tx.open("localhost:19871", 15));

    // Build and send a PPKT packet with 4 float samples
    PpktHeader hdr = ppkt_make_header(DTYPE_F32, 0);
    hdr.sample_count = 4;
    hdr.payload_bytes = 16;
    hdr.sample_rate_hz = 1000.0;
    hdr.timestamp_ns = pipit_now_ns();
    hdr.iteration_index = 0;

    float samples[4] = {1.0f, 2.0f, 3.0f, 4.0f};
    uint8_t pkt[sizeof(PpktHeader) + 16];
    std::memcpy(pkt, &hdr, sizeof(PpktHeader));
    std::memcpy(pkt + sizeof(PpktHeader), samples, 16);

    ASSERT_TRUE(tx.send(pkt, sizeof(pkt)));

    // Small delay to allow delivery
    usleep(1000);

    // Receive
    uint8_t recv_buf[256];
    ssize_t n = rx.recv(recv_buf, sizeof(recv_buf));
    ASSERT_TRUE(n == static_cast<ssize_t>(sizeof(pkt)));

    // Validate header
    PpktHeader recv_hdr;
    std::memcpy(&recv_hdr, recv_buf, sizeof(PpktHeader));
    ASSERT_TRUE(ppkt_validate(recv_hdr));
    ASSERT_EQ(recv_hdr.sample_count, 4);
    ASSERT_EQ(recv_hdr.payload_bytes, 16);
    ASSERT_EQ(recv_hdr.sample_rate_hz, 1000.0);

    // Validate payload
    float recv_samples[4];
    std::memcpy(recv_samples, recv_buf + sizeof(PpktHeader), 16);
    ASSERT_EQ(recv_samples[0], 1.0f);
    ASSERT_EQ(recv_samples[1], 2.0f);
    ASSERT_EQ(recv_samples[2], 3.0f);
    ASSERT_EQ(recv_samples[3], 4.0f);
}

TEST(udp_nonblocking_recv_no_data) {
    DatagramReceiver rx;
    ASSERT_TRUE(rx.open("localhost:19872", 15));

    // recv should return 0 immediately (no data)
    uint8_t buf[256];
    ssize_t n = rx.recv(buf, sizeof(buf));
    ASSERT_EQ(n, 0);
}

// ── Chunked send test ──

TEST(ppkt_send_chunked_single) {
    // When N fits in one MTU, only one packet is sent
    DatagramReceiver rx;
    ASSERT_TRUE(rx.open("localhost:19873", 15));

    DatagramSender tx;
    ASSERT_TRUE(tx.open("localhost:19873", 15));

    PpktHeader hdr = ppkt_make_header(DTYPE_F32, 0);
    hdr.sample_rate_hz = 48000.0;
    hdr.timestamp_ns = pipit_now_ns();
    hdr.iteration_index = 100;

    float samples[10];
    for (int i = 0; i < 10; i++)
        samples[i] = static_cast<float>(i);

    int sent = ppkt_send_chunked(tx, hdr, samples, 10);
    ASSERT_EQ(sent, 1);

    usleep(1000);

    uint8_t buf[512];
    ssize_t n = rx.recv(buf, sizeof(buf));
    ASSERT_TRUE(n > 0);

    PpktHeader recv_hdr;
    std::memcpy(&recv_hdr, buf, sizeof(PpktHeader));
    ASSERT_EQ(recv_hdr.sample_count, 10);
    ASSERT_EQ(recv_hdr.payload_bytes, 40);
    ASSERT_EQ(recv_hdr.iteration_index, 100);
}

TEST(ppkt_send_chunked_multiple) {
    // Force chunking by setting a very small MTU
    DatagramReceiver rx;
    ASSERT_TRUE(rx.open("localhost:19874", 15));

    DatagramSender tx;
    ASSERT_TRUE(tx.open("localhost:19874", 15));

    PpktHeader hdr = ppkt_make_header(DTYPE_F32, 0);
    hdr.sample_rate_hz = 1000.0;
    hdr.timestamp_ns = pipit_now_ns();
    hdr.iteration_index = 0;

    float samples[20];
    for (int i = 0; i < 20; i++)
        samples[i] = static_cast<float>(i);

    // MTU = 48 + 32 = 80 bytes → max 8 float samples per packet
    // 20 samples → 3 packets (8 + 8 + 4)
    size_t tiny_mtu = sizeof(PpktHeader) + 32;
    int sent = ppkt_send_chunked(tx, hdr, samples, 20, tiny_mtu);
    ASSERT_EQ(sent, 3);

    usleep(1000);

    // Read first chunk
    uint8_t buf[512];
    ssize_t n = rx.recv(buf, sizeof(buf));
    ASSERT_TRUE(n > 0);
    PpktHeader h1;
    std::memcpy(&h1, buf, sizeof(PpktHeader));
    ASSERT_EQ(h1.sample_count, 8);
    ASSERT_EQ(h1.iteration_index, 0);
    ASSERT_EQ(h1.sequence, 0);

    // Second chunk
    n = rx.recv(buf, sizeof(buf));
    ASSERT_TRUE(n > 0);
    PpktHeader h2;
    std::memcpy(&h2, buf, sizeof(PpktHeader));
    ASSERT_EQ(h2.sample_count, 8);
    ASSERT_EQ(h2.iteration_index, 8);
    ASSERT_EQ(h2.sequence, 1);

    // Third chunk
    n = rx.recv(buf, sizeof(buf));
    ASSERT_TRUE(n > 0);
    PpktHeader h3;
    std::memcpy(&h3, buf, sizeof(PpktHeader));
    ASSERT_EQ(h3.sample_count, 4);
    ASSERT_EQ(h3.iteration_index, 16);
    ASSERT_EQ(h3.sequence, 2);
}

TEST(ppkt_send_chunked_sequence_continuity) {
    // Sequence number should be updated in the header after chunked send
    PpktHeader hdr = ppkt_make_header(DTYPE_F32, 5);
    hdr.sequence = 10;

    // No actual send needed — just verify sequence update
    // After sending 20 samples with tiny MTU (3 chunks), sequence should be 13
    DatagramSender tx;
    ASSERT_TRUE(tx.open("localhost:19875", 15));

    float samples[20];
    for (int i = 0; i < 20; i++)
        samples[i] = 0.0f;

    size_t tiny_mtu = sizeof(PpktHeader) + 32;
    ppkt_send_chunked(tx, hdr, samples, 20, tiny_mtu);
    ASSERT_EQ(hdr.sequence, 13); // 10 + 3 chunks
}

// ── Main ──

int main() {
    printf("\n=== PPKT / pipit_net.h Tests ===\n\n");
    printf("\nAll tests passed!\n");
    return 0;
}
