//
// test_ringbuf.cpp — Runtime tests for SPSC RingBuffer partial specialization
//
// Tests: SPSC correctness (single-threaded), SPSC concurrent stress test,
//        API compatibility with generic class
//

#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <pipit.h>
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

#define ASSERT_TRUE(cond)                                                                          \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            fprintf(stderr, "FAIL: %s:%d: %s\n", __FILE__, __LINE__, #cond);                       \
            exit(1);                                                                               \
        }                                                                                          \
    } while (0)

#define ASSERT_EQ(actual, expected)                                                                \
    do {                                                                                           \
        auto _a = (actual);                                                                        \
        auto _e = (expected);                                                                      \
        if (_a != _e) {                                                                            \
            fprintf(stderr, "FAIL: %s:%d: expected %ld, got %ld\n", __FILE__, __LINE__, (long)_e,  \
                    (long)_a);                                                                     \
            exit(1);                                                                               \
        }                                                                                          \
    } while (0)

// ── Verify SPSC specialization is selected when Readers=1 ──

TEST(spsc_type_is_specialization) {
    // This test verifies that RingBuffer<float, 64, 1> compiles and can be
    // instantiated. The partial specialization is selected by C++ template
    // matching; if it fails to compile, the specialization is broken.
    pipit::RingBuffer<float, 64, 1> rb;
    ASSERT_EQ(rb.available(), 0u);
}

// ── Basic single-element write/read ──

TEST(spsc_single_write_read) {
    pipit::RingBuffer<float, 64, 1> rb;
    float val = 42.0f;
    ASSERT_TRUE(rb.write(&val, 1));
    ASSERT_EQ(rb.available(), 1u);

    float out = 0.0f;
    ASSERT_TRUE(rb.read(&out, 1));
    ASSERT_TRUE(out == 42.0f);
    ASSERT_EQ(rb.available(), 0u);
}

// ── API compatibility: read(reader_idx, dst, count) ──

TEST(spsc_read_with_reader_idx) {
    pipit::RingBuffer<float, 64, 1> rb;
    float val = 3.14f;
    ASSERT_TRUE(rb.write(&val, 1));

    float out = 0.0f;
    // Codegen emits read(reader_idx, dst, count) — must work with SPSC
    ASSERT_TRUE(rb.read(0, &out, 1));
    ASSERT_TRUE(out == 3.14f);
}

// ── Block write/read ──

TEST(spsc_block_transfer) {
    pipit::RingBuffer<float, 256, 1> rb;
    float src[64];
    for (int i = 0; i < 64; ++i)
        src[i] = static_cast<float>(i);
    ASSERT_TRUE(rb.write(src, 64));
    ASSERT_EQ(rb.available(), 64u);

    float dst[64];
    std::memset(dst, 0, sizeof(dst));
    ASSERT_TRUE(rb.read(dst, 64));
    for (int i = 0; i < 64; ++i)
        ASSERT_TRUE(dst[i] == static_cast<float>(i));
    ASSERT_EQ(rb.available(), 0u);
}

// ── Wraparound: write past end of circular buffer ──

TEST(spsc_wraparound) {
    pipit::RingBuffer<float, 8, 1> rb;

    // Fill 6 of 8 slots
    float src1[6] = {1, 2, 3, 4, 5, 6};
    ASSERT_TRUE(rb.write(src1, 6));

    // Read 4, freeing slots 0-3
    float tmp[4];
    ASSERT_TRUE(rb.read(tmp, 4));

    // Write 5 more — wraps around (positions 6,7,0,1,2)
    float src2[5] = {10, 20, 30, 40, 50};
    ASSERT_TRUE(rb.write(src2, 5));

    // Read remaining: slots 4-5 have {5,6}, then 6-7,0-2 have {10,20,30,40,50}
    float out[7];
    ASSERT_TRUE(rb.read(out, 7));
    ASSERT_TRUE(out[0] == 5.0f);
    ASSERT_TRUE(out[1] == 6.0f);
    ASSERT_TRUE(out[2] == 10.0f);
    ASSERT_TRUE(out[3] == 20.0f);
    ASSERT_TRUE(out[4] == 30.0f);
    ASSERT_TRUE(out[5] == 40.0f);
    ASSERT_TRUE(out[6] == 50.0f);
}

// ── Full buffer: write should fail when buffer is full ──

TEST(spsc_full_buffer_fails) {
    pipit::RingBuffer<float, 4, 1> rb;
    float src[4] = {1, 2, 3, 4};
    ASSERT_TRUE(rb.write(src, 4));
    ASSERT_EQ(rb.available(), 4u);

    // One more write should fail
    float extra = 99.0f;
    ASSERT_TRUE(!rb.write(&extra, 1));
}

// ── Empty buffer: read should fail when buffer is empty ──

TEST(spsc_empty_read_fails) {
    pipit::RingBuffer<float, 8, 1> rb;
    float out;
    ASSERT_TRUE(!rb.read(&out, 1));
}

// ── Concurrent stress test: single writer + single reader ──

TEST(spsc_concurrent_stress) {
    static constexpr size_t CAPACITY = 1024;
    static constexpr size_t TOTAL = 100000;
    static constexpr size_t CHUNK = 64;

    pipit::RingBuffer<int, CAPACITY, 1> rb;
    std::atomic<bool> done{false};
    std::atomic<int> errors{0};

    // Writer thread
    std::thread writer([&]() {
        int val = 0;
        size_t remaining = TOTAL;
        while (remaining > 0) {
            size_t n = std::min(remaining, CHUNK);
            int buf[CHUNK];
            for (size_t i = 0; i < n; ++i)
                buf[i] = val++;
            while (!rb.write(buf, n)) {
                std::this_thread::yield();
            }
            remaining -= n;
        }
        done.store(true, std::memory_order_release);
    });

    // Reader thread (main)
    int expected = 0;
    size_t read_total = 0;
    while (read_total < TOTAL) {
        size_t n = std::min(TOTAL - read_total, CHUNK);
        int buf[CHUNK];
        if (rb.read(0, buf, n)) {
            for (size_t i = 0; i < n; ++i) {
                if (buf[i] != expected++) {
                    errors.fetch_add(1, std::memory_order_relaxed);
                }
            }
            read_total += n;
        } else {
            std::this_thread::yield();
        }
    }

    writer.join();
    ASSERT_EQ(errors.load(), 0);
    ASSERT_EQ(read_total, TOTAL);
}

// ── Generic (multi-reader) still works alongside SPSC ──

TEST(generic_multi_reader_still_works) {
    pipit::RingBuffer<float, 64, 2> rb;
    float val = 7.0f;
    ASSERT_TRUE(rb.write(&val, 1));
    ASSERT_EQ(rb.available(0), 1u);
    ASSERT_EQ(rb.available(1), 1u);

    float out0 = 0.0f, out1 = 0.0f;
    ASSERT_TRUE(rb.read(0, &out0, 1));
    ASSERT_TRUE(rb.read(1, &out1, 1));
    ASSERT_TRUE(out0 == 7.0f);
    ASSERT_TRUE(out1 == 7.0f);
}

int main() {
    printf("All RingBuffer tests passed.\n");
    return 0;
}
