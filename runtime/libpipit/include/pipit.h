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

// ── Ring buffer (lock-free single-writer, multi-reader) ─────────────────────

namespace pipit {

template <typename T, std::size_t Capacity, std::size_t Readers = 1> class RingBuffer {
    static_assert(Capacity > 0, "RingBuffer capacity must be > 0");
    static_assert(Readers > 0, "RingBuffer must have at least one reader");
    static constexpr std::size_t N = Capacity;

    alignas(64) std::atomic<std::size_t> head_{0};          // absolute write cursor
    alignas(64) std::atomic<std::size_t> tails_[Readers]{}; // absolute read cursors
    T buf_[N];

  public:
    RingBuffer() = default;

    bool write(const T *src, std::size_t count) {
        std::size_t h = head_.load(std::memory_order_relaxed);
        std::size_t min_tail = tails_[0].load(std::memory_order_acquire);
        for (std::size_t i = 1; i < Readers; ++i) {
            std::size_t t = tails_[i].load(std::memory_order_acquire);
            if (t < min_tail)
                min_tail = t;
        }
        std::size_t used = h - min_tail;
        if (used > Capacity)
            return false;
        std::size_t free = Capacity - used;
        if (count > free)
            return false;
        for (std::size_t i = 0; i < count; ++i) {
            buf_[(h + i) % N] = src[i];
        }
        head_.store(h + count, std::memory_order_release);
        return true;
    }

    bool read(std::size_t reader_idx, T *dst, std::size_t count) {
        if (reader_idx >= Readers)
            return false;
        std::size_t t = tails_[reader_idx].load(std::memory_order_relaxed);
        std::size_t h = head_.load(std::memory_order_acquire);
        std::size_t avail = h - t;
        if (count > avail)
            return false;
        for (std::size_t i = 0; i < count; ++i) {
            dst[i] = buf_[(t + i) % N];
        }
        tails_[reader_idx].store(t + count, std::memory_order_release);
        return true;
    }

    bool read(T *dst, std::size_t count) { return read(0, dst, count); }

    std::size_t available(std::size_t reader_idx = 0) const {
        if (reader_idx >= Readers)
            return 0;
        std::size_t h = head_.load(std::memory_order_acquire);
        std::size_t t = tails_[reader_idx].load(std::memory_order_acquire);
        return h - t;
    }
};

// ── Timer (chrono-based tick generator) ─────────────────────────────────────

class Timer {
    using Clock = std::chrono::steady_clock;
    using Nanos = std::chrono::nanoseconds;

    Nanos period_;
    Clock::time_point next_;
    bool overrun_ = false;
    Nanos last_latency_{0};
    bool measure_latency_;
    Nanos spin_threshold_{0};

  public:
    explicit Timer(double freq_hz, bool measure_latency = true, int64_t spin_ns = 0)
        : period_(std::chrono::duration_cast<Nanos>(std::chrono::duration<double>(1.0 / freq_hz))),
          next_(Clock::now() + period_), measure_latency_(measure_latency),
          spin_threshold_(spin_ns) {}

    void wait() {
        auto now = Clock::now();
        if (now < next_) {
            if (spin_threshold_.count() > 0) {
                // Hybrid: sleep for bulk of the period, spin for the final portion
                auto sleep_target = next_ - spin_threshold_;
                if (now < sleep_target) {
                    std::this_thread::sleep_until(sleep_target);
                }
                while (Clock::now() < next_) { /* spin */
                }
            } else {
                std::this_thread::sleep_until(next_);
            }
            overrun_ = false;
            if (measure_latency_) {
                last_latency_ = Clock::now() - next_;
            }
        } else {
            overrun_ = true;
            if (measure_latency_) {
                last_latency_ = now - next_;
            }
        }
        next_ += period_;
    }

    bool overrun() const { return overrun_; }

    Nanos last_latency() const { return last_latency_; }

    // For backlog policy: how many ticks we've fallen behind
    int64_t missed_count() const {
        auto now = Clock::now();
        if (now < next_)
            return 0;
        return static_cast<int64_t>((now - next_).count() / period_.count()) + 1;
    }

    // For slip policy: re-anchor to current time
    void reset_phase() {
        next_ = Clock::now() + period_;
        overrun_ = false;
    }
};

// ── Statistics collection ────────────────────────────────────────────────────

struct TaskStats {
    uint64_t ticks = 0;
    uint64_t missed = 0;
    int64_t max_latency_ns = 0;
    int64_t total_latency_ns = 0;

    void record_tick(std::chrono::nanoseconds latency) {
        ++ticks;
        auto ns = latency.count();
        if (ns > max_latency_ns)
            max_latency_ns = ns;
        total_latency_ns += ns;
    }

    void record_miss() { ++missed; }

    int64_t avg_latency_ns() const {
        return ticks > 0 ? total_latency_ns / static_cast<int64_t>(ticks) : 0;
    }
};

} // namespace pipit
