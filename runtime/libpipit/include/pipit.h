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
        int operator()(const _PIPIT_FIRST(in_spec) * in, _PIPIT_FIRST(out_spec) * out) noexcept

// ── Ring buffer (lock-free single-writer, multi-reader) ─────────────────────

namespace pipit {

namespace detail {

struct ActorRuntimeContext {
    uint64_t iteration_index = 0;
    double task_rate_hz = 0.0;
};

inline ActorRuntimeContext &actor_runtime_context() {
    static thread_local ActorRuntimeContext ctx{};
    return ctx;
}

inline void set_actor_iteration_index(uint64_t iteration_index) {
    actor_runtime_context().iteration_index = iteration_index;
}

inline void set_actor_task_rate_hz(double task_rate_hz) {
    actor_runtime_context().task_rate_hz = task_rate_hz;
}

} // namespace detail

template <typename T, std::size_t Capacity, std::size_t Readers = 1> class RingBuffer {
    static_assert(Capacity > 0, "RingBuffer capacity must be > 0");
    static_assert(Readers > 0, "RingBuffer must have at least one reader");
    static_assert(std::is_trivially_copyable_v<T>,
                  "RingBuffer element type must be trivially copyable");
    static constexpr std::size_t N = Capacity;

    struct alignas(64) PaddedTail {
        std::atomic<std::size_t> value{0};
    };

    alignas(64) std::atomic<std::size_t> head_{0}; // absolute write cursor
    PaddedTail tails_[Readers];                    // absolute read cursors (cache-line isolated)
    std::size_t cached_min_tail_{0};               // writer-private cached min tail
    T buf_[N];

  public:
    RingBuffer() = default;

    bool write(const T *src, std::size_t count) {
        std::size_t h = head_.load(std::memory_order_relaxed);
        // Fast path: check with cached min_tail (avoids O(Readers) acquire loads)
        std::size_t used = h - cached_min_tail_;
        if (used > Capacity || Capacity - used < count) {
            // Slow path: rescan all tails
            std::size_t mt = tails_[0].value.load(std::memory_order_acquire);
            for (std::size_t i = 1; i < Readers; ++i) {
                std::size_t t = tails_[i].value.load(std::memory_order_acquire);
                if (t < mt)
                    mt = t;
            }
            cached_min_tail_ = mt;
            used = h - cached_min_tail_;
            if (used > Capacity)
                return false;
            if (Capacity - used < count)
                return false;
        }
        // Two-phase memcpy (avoids per-element modulo)
        std::size_t start = h % N;
        std::size_t first = std::min(count, N - start);
        std::memcpy(&buf_[start], src, first * sizeof(T));
        if (first < count)
            std::memcpy(&buf_[0], src + first, (count - first) * sizeof(T));
        head_.store(h + count, std::memory_order_release);
        return true;
    }

    bool read(std::size_t reader_idx, T *dst, std::size_t count) {
        if (reader_idx >= Readers)
            return false;
        std::size_t t = tails_[reader_idx].value.load(std::memory_order_relaxed);
        std::size_t h = head_.load(std::memory_order_acquire);
        std::size_t avail = h - t;
        if (count > avail)
            return false;
        // Two-phase memcpy (avoids per-element modulo)
        std::size_t start = t % N;
        std::size_t first = std::min(count, N - start);
        std::memcpy(dst, &buf_[start], first * sizeof(T));
        if (first < count)
            std::memcpy(dst + first, &buf_[0], (count - first) * sizeof(T));
        tails_[reader_idx].value.store(t + count, std::memory_order_release);
        return true;
    }

    bool read(T *dst, std::size_t count) { return read(0, dst, count); }

    std::size_t available(std::size_t reader_idx = 0) const {
        if (reader_idx >= Readers)
            return 0;
        std::size_t h = head_.load(std::memory_order_acquire);
        std::size_t t = tails_[reader_idx].value.load(std::memory_order_acquire);
        return h - t;
    }
};

// ── SPSC partial specialization (ADR-029) ───────────────────────────────────
//
// When Readers == 1, the generic multi-reader tail scan loop is unnecessary.
// This specialization uses a single tail and writer-private cached_tail for
// the fast path, preserving the same API surface and memory ordering model.

template <typename T, std::size_t Capacity> class RingBuffer<T, Capacity, 1> {
    static_assert(Capacity > 0, "RingBuffer capacity must be > 0");
    static_assert(std::is_trivially_copyable_v<T>,
                  "RingBuffer element type must be trivially copyable");
    static constexpr std::size_t N = Capacity;

    struct alignas(64) PaddedTail {
        std::atomic<std::size_t> value{0};
    };

    alignas(64) std::atomic<std::size_t> head_{0}; // absolute write cursor
    PaddedTail tail_;                              // single reader tail
    std::size_t cached_tail_{0};                   // writer-private cached tail
    T buf_[N];

  public:
    RingBuffer() = default;

    bool write(const T *src, std::size_t count) {
        std::size_t h = head_.load(std::memory_order_relaxed);
        std::size_t used = h - cached_tail_;
        if (used > Capacity || Capacity - used < count) {
            // Slow path: reload single tail
            cached_tail_ = tail_.value.load(std::memory_order_acquire);
            used = h - cached_tail_;
            if (used > Capacity)
                return false;
            if (Capacity - used < count)
                return false;
        }
        std::size_t start = h % N;
        std::size_t first = std::min(count, N - start);
        std::memcpy(&buf_[start], src, first * sizeof(T));
        if (first < count)
            std::memcpy(&buf_[0], src + first, (count - first) * sizeof(T));
        head_.store(h + count, std::memory_order_release);
        return true;
    }

    bool read(std::size_t reader_idx, T *dst, std::size_t count) {
        (void)reader_idx; // single reader — always index 0
        std::size_t t = tail_.value.load(std::memory_order_relaxed);
        std::size_t h = head_.load(std::memory_order_acquire);
        std::size_t avail = h - t;
        if (count > avail)
            return false;
        std::size_t start = t % N;
        std::size_t first = std::min(count, N - start);
        std::memcpy(dst, &buf_[start], first * sizeof(T));
        if (first < count)
            std::memcpy(dst + first, &buf_[0], (count - first) * sizeof(T));
        tail_.value.store(t + count, std::memory_order_release);
        return true;
    }

    bool read(T *dst, std::size_t count) { return read(0, dst, count); }

    std::size_t available(std::size_t reader_idx = 0) const {
        (void)reader_idx;
        std::size_t h = head_.load(std::memory_order_acquire);
        std::size_t t = tail_.value.load(std::memory_order_acquire);
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

    // Adaptive spin state (EWMA-based jitter calibration, ADR-014)
    bool adaptive_ = false;
    Nanos ewma_jitter_{0};
    static constexpr int64_t kMinSpinNs = 500;     // floor: 500ns
    static constexpr int64_t kMaxSpinNs = 100'000; // ceiling: 100us
    static constexpr int64_t kInitSpinNs = 10'000; // bootstrap: 10us

  public:
    explicit Timer(double freq_hz, bool measure_latency = true, int64_t spin_ns = 0)
        : period_(std::chrono::duration_cast<Nanos>(std::chrono::duration<double>(1.0 / freq_hz))),
          next_(Clock::now() + period_), measure_latency_(measure_latency) {
        if (spin_ns < 0) {
            // Adaptive mode: EWMA calibration (sentinel -1)
            adaptive_ = true;
            spin_threshold_ = Nanos{kInitSpinNs};
        } else {
            spin_threshold_ = Nanos{spin_ns};
        }
    }

    void wait() {
        auto now = Clock::now();
        if (now < next_) {
            if (spin_threshold_.count() > 0) {
                // Hybrid: sleep for bulk of the period, spin for the final portion
                auto sleep_target = next_ - spin_threshold_;
                if (now < sleep_target) {
                    std::this_thread::sleep_until(sleep_target);
                }
                // Record wake point for adaptive calibration
                auto wake_point = Clock::now();
                // Spin to deadline
                while (Clock::now() < next_) { /* spin */
                }

                if (adaptive_) {
                    // Jitter = how late we woke vs requested sleep_target
                    auto jitter_ns =
                        std::chrono::duration_cast<Nanos>(wake_point - sleep_target).count();
                    if (jitter_ns < 0)
                        jitter_ns = 0;
                    // EWMA update: ewma += (sample - ewma) / 8  (alpha = 1/8)
                    auto delta = jitter_ns - ewma_jitter_.count();
                    ewma_jitter_ += Nanos{delta / 8};
                    // spin_threshold = clamp(2 * ewma, min, max)
                    auto new_spin = ewma_jitter_.count() * 2;
                    if (new_spin < kMinSpinNs)
                        new_spin = kMinSpinNs;
                    if (new_spin > kMaxSpinNs)
                        new_spin = kMaxSpinNs;
                    spin_threshold_ = Nanos{new_spin};
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

    // Adaptive spin observability
    bool is_adaptive() const { return adaptive_; }
    Nanos current_spin_threshold() const { return spin_threshold_; }

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

// ── Actor runtime context API ────────────────────────────────────────────────

inline uint64_t pipit_now_ns() {
    return static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(
                                     std::chrono::steady_clock::now().time_since_epoch())
                                     .count());
}

inline uint64_t pipit_iteration_index() {
    return pipit::detail::actor_runtime_context().iteration_index;
}

inline double pipit_task_rate_hz() { return pipit::detail::actor_runtime_context().task_rate_hz; }
