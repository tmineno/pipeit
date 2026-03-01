#pragma once
/// @file types.h
/// @brief Shared data types for pipscope data sources (PPKT, SHM)
///
/// Contains sample buffer, channel snapshot, frame stats, receiver metrics,
/// and dtype conversion functions used by both PpktReceiver and ShmReceiver.

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

#include <pipit_net.h>

namespace pipscope {

// ── SampleBuffer ─────────────────────────────────────────────────────────────

/// Thread-unsafe circular buffer for float samples.
/// Caller must hold a lock when calling push() or snapshot().
struct SampleBuffer {
    std::vector<float> data;
    size_t capacity;
    size_t head = 0;  // next write position
    size_t count = 0; // valid samples (≤ capacity)

    explicit SampleBuffer(size_t cap = 1'000'000) : data(cap, 0.0f), capacity(cap) {}

    /// Append n samples to the buffer (wrap-aware memcpy).
    void push(const float *samples, size_t n) {
        if (n >= capacity) {
            // More samples than capacity — keep only the last `capacity` samples
            std::memcpy(data.data(), samples + (n - capacity), capacity * sizeof(float));
            head = 0;
            count = capacity;
            return;
        }
        size_t first = std::min(n, capacity - head);
        std::memcpy(data.data() + head, samples, first * sizeof(float));
        if (first < n) {
            std::memcpy(data.data(), samples + first, (n - first) * sizeof(float));
        }
        head = (head + n) % capacity;
        count = std::min(count + n, capacity);
    }

    /// Reset the buffer without reallocating. O(1).
    void clear() {
        head = 0;
        count = 0;
    }

    /// Copy the most recent `max_n` samples into dst (oldest → newest order).
    /// Returns the number of samples actually copied (≤ min(max_n, count)).
    size_t snapshot(float *dst, size_t max_n) const {
        size_t n = std::min(max_n, count);
        if (n == 0)
            return 0;
        // oldest sample is at (head - count) mod capacity
        // we want the last n samples: start at (head - n) mod capacity
        size_t start = (head + capacity - n) % capacity;
        if (start + n <= capacity) {
            std::memcpy(dst, data.data() + start, n * sizeof(float));
        } else {
            size_t first = capacity - start;
            std::memcpy(dst, data.data() + start, first * sizeof(float));
            std::memcpy(dst + first, data.data(), (n - first) * sizeof(float));
        }
        return n;
    }
};

// ── DType → float conversion ─────────────────────────────────────────────────

inline size_t dtype_sample_bytes(uint8_t dtype) {
    switch (static_cast<pipit::net::DType>(dtype)) {
    case pipit::net::DTYPE_F32:
    case pipit::net::DTYPE_I32:
        return 4;
    case pipit::net::DTYPE_CF32:
    case pipit::net::DTYPE_F64:
        return 8;
    case pipit::net::DTYPE_I16:
        return 2;
    case pipit::net::DTYPE_I8:
        return 1;
    default:
        return 0;
    }
}

template <typename T>
inline size_t convert_scalar_to_float(const uint8_t *payload, size_t sample_count, float *out) {
    for (size_t i = 0; i < sample_count; i++) {
        T value{};
        std::memcpy(&value, payload + i * sizeof(T), sizeof(T));
        out[i] = static_cast<float>(value);
    }
    return sample_count;
}

inline size_t convert_f32_to_float(const uint8_t *payload, size_t sample_count, float *out) {
    std::memcpy(out, payload, sample_count * sizeof(float));
    return sample_count;
}

inline size_t convert_cf32_to_magnitude(const uint8_t *payload, size_t sample_count, float *out) {
    for (size_t i = 0; i < sample_count; i++) {
        float re = 0.0f;
        float im = 0.0f;
        std::memcpy(&re, payload + i * 8, 4);
        std::memcpy(&im, payload + i * 8 + 4, 4);
        out[i] = std::sqrt(re * re + im * im);
    }
    return sample_count;
}

/// Convert PPKT/PSHM payload of any dtype to float samples.
/// Returns number of float samples written to out.
inline size_t convert_to_float(const uint8_t *payload, uint32_t sample_count, uint8_t dtype,
                               float *out) {
    switch (static_cast<pipit::net::DType>(dtype)) {
    case pipit::net::DTYPE_F32:
        return convert_f32_to_float(payload, sample_count, out);
    case pipit::net::DTYPE_I32:
        return convert_scalar_to_float<int32_t>(payload, sample_count, out);
    case pipit::net::DTYPE_CF32:
        return convert_cf32_to_magnitude(payload, sample_count, out);
    case pipit::net::DTYPE_F64:
        return convert_scalar_to_float<double>(payload, sample_count, out);
    case pipit::net::DTYPE_I16:
        return convert_scalar_to_float<int16_t>(payload, sample_count, out);
    case pipit::net::DTYPE_I8:
        return convert_scalar_to_float<int8_t>(payload, sample_count, out);
    default:
        return 0;
    }
}

/// Bounded conversion that never reads beyond `payload_bytes`.
inline size_t convert_to_float(const uint8_t *payload, size_t payload_bytes, uint32_t sample_count,
                               uint8_t dtype, float *out) {
    size_t sample_bytes = dtype_sample_bytes(dtype);
    if (sample_bytes == 0) {
        return 0;
    }
    size_t bounded_samples = std::min<size_t>(sample_count, payload_bytes / sample_bytes);
    return convert_to_float(payload, static_cast<uint32_t>(bounded_samples), dtype, out);
}

// ── Frame integrity stats ────────────────────────────────────────────────────

struct FrameStats {
    uint64_t accepted_frames = 0;
    uint64_t dropped_frames = 0;
    uint64_t drop_seq_gap = 0;       // sequence discontinuity
    uint64_t drop_iter_gap = 0;      // iteration_index discontinuity
    uint64_t drop_boundary = 0;      // missing start/end boundary
    uint64_t drop_meta_mismatch = 0; // dtype/sample_rate changed mid-frame
    uint64_t inter_frame_gaps = 0;   // kernel-level packet loss (inter-frame iter gap)
};

// ── ChannelSnapshot ──────────────────────────────────────────────────────────

/// Snapshot of a single channel's state (thread-safe copy for rendering).
struct ChannelSnapshot {
    uint16_t chan_id = 0;
    double sample_rate_hz = 0.0;
    uint64_t packet_count = 0;
    FrameStats stats;
    std::vector<float> samples;
    std::string label; // human-readable label (e.g., "shm:rx.iq"); empty = use "Ch %u"
};

/// Receiver-level metrics for observability (lock-free, atomic reads).
struct ReceiverMetrics {
    uint64_t recv_packets;
    uint64_t recv_bytes;
};

} // namespace pipscope
