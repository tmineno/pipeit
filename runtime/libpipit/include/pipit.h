#pragma once

// pipit.h — Pipit runtime library
//
// This header provides the ACTOR macro, ring buffer, and timer for Pipit
// pipelines. Used by both hand-written actor headers and generated code.

#include <atomic>
#include <chrono>
#include <complex>
#include <cstdint>
#include <cstring>
#include <span>
#include <thread>

// Actor return codes
constexpr int ACTOR_OK = 0;
constexpr int ACTOR_ERROR = 1;

// Type aliases used in actor definitions
using cfloat = std::complex<float>;
using cdouble = std::complex<double>;
using int32 = std::int32_t;

// ── Actor registration macros ───────────────────────────────────────────────
//
// ACTOR(name, IN(type, count), OUT(type, count), [PARAM|RUNTIME_PARAM]...)
//
// IN/OUT expand to (type, count) pairs — consumed by the compiler scanner.
// PARAM/RUNTIME_PARAM declare member variables in the actor struct.
// The actor body becomes the operator() of the generated struct.
//
// The _PIPIT_FIRST helper extracts the type from the expanded IN/OUT pair
// to produce typed `in`/`out` pointers in operator().

#define IN(type, count) type, count
#define OUT(type, count) type, count
#define PARAM(type, name) type name;
#define RUNTIME_PARAM(type, name) type name;

// Helper: extract the first element from a comma-separated pair
#define _PIPIT_FIRST(a, ...) a

// ACTOR macro: generates a struct with typed operator()
// IN/OUT are expanded during argument prescan, so in_spec becomes "type, count".
// _PIPIT_FIRST extracts the type for the pointer declaration.
#define ACTOR(name, in_spec, out_spec, ...)                                                        \
    struct Actor_##name {                                                                          \
        __VA_ARGS__                                                                                \
        int operator()(const _PIPIT_FIRST(in_spec) * in, _PIPIT_FIRST(out_spec) * out)

// ── Ring buffer (lock-free SPSC) ────────────────────────────────────────────

namespace pipit {

template <typename T, std::size_t Capacity> class RingBuffer {
    static_assert(Capacity > 0, "RingBuffer capacity must be > 0");
    static constexpr std::size_t N = Capacity + 1; // one extra slot for full/empty

    alignas(64) std::atomic<std::size_t> head_{0};
    alignas(64) std::atomic<std::size_t> tail_{0};
    T buf_[N];

  public:
    RingBuffer() = default;

    bool write(const T *src, std::size_t count) {
        std::size_t h = head_.load(std::memory_order_relaxed);
        std::size_t t = tail_.load(std::memory_order_acquire);
        std::size_t free = (t - h - 1 + N) % N;
        if (count > free)
            return false;
        for (std::size_t i = 0; i < count; ++i) {
            buf_[(h + i) % N] = src[i];
        }
        head_.store((h + count) % N, std::memory_order_release);
        return true;
    }

    bool read(T *dst, std::size_t count) {
        std::size_t t = tail_.load(std::memory_order_relaxed);
        std::size_t h = head_.load(std::memory_order_acquire);
        std::size_t avail = (h - t + N) % N;
        if (count > avail)
            return false;
        for (std::size_t i = 0; i < count; ++i) {
            dst[i] = buf_[(t + i) % N];
        }
        tail_.store((t + count) % N, std::memory_order_release);
        return true;
    }

    std::size_t available() const {
        std::size_t h = head_.load(std::memory_order_acquire);
        std::size_t t = tail_.load(std::memory_order_acquire);
        return (h - t + N) % N;
    }
};

// ── Timer (chrono-based tick generator) ─────────────────────────────────────

class Timer {
    using Clock = std::chrono::steady_clock;
    using Nanos = std::chrono::nanoseconds;

    Nanos period_;
    Clock::time_point next_;
    bool overrun_ = false;

  public:
    explicit Timer(double freq_hz)
        : period_(std::chrono::duration_cast<Nanos>(std::chrono::duration<double>(1.0 / freq_hz))),
          next_(Clock::now() + period_) {}

    void wait() {
        auto now = Clock::now();
        if (now < next_) {
            std::this_thread::sleep_until(next_);
            overrun_ = false;
        } else {
            overrun_ = true;
        }
        next_ += period_;
    }

    bool overrun() const { return overrun_; }
};

} // namespace pipit
