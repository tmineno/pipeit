#pragma once
/// @file ppkt_receiver.h
/// @brief PPKT packet receiver with per-channel sample buffers for pipscope

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <map>
#include <mutex>
#include <thread>
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

    /// Append n samples to the buffer.
    void push(const float *samples, size_t n) {
        for (size_t i = 0; i < n; i++) {
            data[head] = samples[i];
            head = (head + 1) % capacity;
        }
        count = std::min(count + n, capacity);
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

/// Convert PPKT payload of any dtype to float samples.
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

// ── ChannelState ─────────────────────────────────────────────────────────────

struct ChannelState {
    uint16_t chan_id;
    double sample_rate_hz = 0.0;
    uint32_t last_sequence = 0;
    uint64_t packet_count = 0;
    SampleBuffer buffer;

    explicit ChannelState(uint16_t id, size_t buf_capacity) : chan_id(id), buffer(buf_capacity) {}
};

// ── PpktReceiver ─────────────────────────────────────────────────────────────

/// Snapshot of a single channel's state (thread-safe copy for rendering).
struct ChannelSnapshot {
    uint16_t chan_id;
    double sample_rate_hz;
    uint64_t packet_count;
    std::vector<float> samples;
};

class PpktReceiver {
    int fd_ = -1;
    std::atomic<bool> running_{false};
    std::thread thread_;

    mutable std::mutex mutex_;
    std::map<uint16_t, ChannelState> channels_;
    size_t buffer_capacity_;

  public:
    explicit PpktReceiver(size_t buffer_capacity = 1'000'000) : buffer_capacity_(buffer_capacity) {}

    ~PpktReceiver() { stop(); }

    /// Bind to UDP port on localhost and start the receiver thread.
    bool start(uint16_t port) {
        // Create and bind socket
        fd_ = ::socket(AF_INET, SOCK_DGRAM, 0);
        if (fd_ < 0)
            return false;

        int optval = 1;
        setsockopt(fd_, SOL_SOCKET, SO_REUSEADDR, &optval, sizeof(optval));

        struct sockaddr_in addr{};
        addr.sin_family = AF_INET;
        addr.sin_addr.s_addr = htonl(INADDR_ANY);
        addr.sin_port = htons(port);

        if (::bind(fd_, reinterpret_cast<struct sockaddr *>(&addr), sizeof(addr)) < 0) {
            ::close(fd_);
            fd_ = -1;
            return false;
        }

        // Set non-blocking
        int flags = fcntl(fd_, F_GETFL, 0);
        if (flags < 0 || fcntl(fd_, F_SETFL, flags | O_NONBLOCK) < 0) {
            ::close(fd_);
            fd_ = -1;
            return false;
        }

        running_.store(true);
        thread_ = std::thread(&PpktReceiver::recv_loop, this);
        return true;
    }

    /// Stop the receiver thread and close the socket.
    void stop() {
        running_.store(false);
        if (thread_.joinable())
            thread_.join();
        if (fd_ >= 0) {
            ::close(fd_);
            fd_ = -1;
        }
    }

    /// Get a snapshot of all channels for rendering.
    std::vector<ChannelSnapshot> snapshot(size_t max_samples) const {
        std::lock_guard<std::mutex> lock(mutex_);
        std::vector<ChannelSnapshot> result;
        result.reserve(channels_.size());

        for (auto &[id, ch] : channels_) {
            ChannelSnapshot snap;
            snap.chan_id = ch.chan_id;
            snap.sample_rate_hz = ch.sample_rate_hz;
            snap.packet_count = ch.packet_count;
            snap.samples.resize(max_samples);
            size_t n = ch.buffer.snapshot(snap.samples.data(), max_samples);
            snap.samples.resize(n);
            result.push_back(std::move(snap));
        }

        return result;
    }

    // Non-copyable
    PpktReceiver(const PpktReceiver &) = delete;
    PpktReceiver &operator=(const PpktReceiver &) = delete;

  private:
    static constexpr size_t kMaxPacketBytes = 65536;
    static constexpr size_t kMaxConvertedSamples = 8192;
    static constexpr auto kPollSleep = std::chrono::microseconds(100);

    enum class RecvStatus { Retry, Packet, Fatal };

    RecvStatus recv_datagram(uint8_t *buf, size_t buf_cap, size_t &bytes_received) const {
        ssize_t n = ::recvfrom(fd_, buf, buf_cap, 0, nullptr, nullptr);
        if (n > 0) {
            bytes_received = static_cast<size_t>(n);
            return RecvStatus::Packet;
        }
        if (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK) {
            return RecvStatus::Fatal;
        }
        std::this_thread::sleep_for(kPollSleep);
        return RecvStatus::Retry;
    }

    bool decode_packet(const uint8_t *packet, size_t packet_size, pipit::net::PpktHeader &hdr,
                       const uint8_t *&payload, size_t &payload_bytes) const {
        if (packet_size < sizeof(pipit::net::PpktHeader)) {
            return false;
        }

        std::memcpy(&hdr, packet, sizeof(pipit::net::PpktHeader));
        if (!pipit::net::ppkt_validate(hdr)) {
            return false;
        }

        size_t payload_offset = sizeof(pipit::net::PpktHeader);
        size_t payload_avail = packet_size - payload_offset;
        if (payload_avail < hdr.payload_bytes) {
            return false;
        }

        payload = packet + payload_offset;
        payload_bytes = hdr.payload_bytes;
        return true;
    }

    bool decode_samples(const uint8_t *payload, size_t payload_bytes,
                        const pipit::net::PpktHeader &hdr, float *conv_buf, size_t conv_capacity,
                        size_t &converted) const {
        size_t bounded_count = std::min<size_t>(hdr.sample_count, conv_capacity);
        converted = convert_to_float(payload, payload_bytes, static_cast<uint32_t>(bounded_count),
                                     hdr.dtype, conv_buf);
        return converted > 0;
    }

    ChannelState &get_or_create_channel(uint16_t chan_id) {
        auto it = channels_.find(chan_id);
        if (it != channels_.end()) {
            return it->second;
        }
        auto [inserted, _] = channels_.emplace(chan_id, ChannelState(chan_id, buffer_capacity_));
        return inserted->second;
    }

    void push_samples(const pipit::net::PpktHeader &hdr, const float *samples,
                      size_t sample_count) {
        std::lock_guard<std::mutex> lock(mutex_);
        auto &ch = get_or_create_channel(hdr.chan_id);
        ch.sample_rate_hz = hdr.sample_rate_hz;
        ch.last_sequence = hdr.sequence;
        ch.packet_count++;
        ch.buffer.push(samples, sample_count);
    }

    void recv_loop() {
        alignas(8) std::array<uint8_t, kMaxPacketBytes> buf{};
        std::array<float, kMaxConvertedSamples> conv_buf{};

        while (running_.load()) {
            size_t packet_size = 0;
            RecvStatus status = recv_datagram(buf.data(), buf.size(), packet_size);
            if (status == RecvStatus::Retry) {
                continue;
            }
            if (status == RecvStatus::Fatal) {
                break;
            }

            pipit::net::PpktHeader hdr{};
            const uint8_t *payload = nullptr;
            size_t payload_bytes = 0;
            if (!decode_packet(buf.data(), packet_size, hdr, payload, payload_bytes)) {
                continue;
            }

            size_t converted = 0;
            if (!decode_samples(payload, payload_bytes, hdr, conv_buf.data(), conv_buf.size(),
                                converted)) {
                continue;
            }

            push_samples(hdr, conv_buf.data(), converted);
        }
    }
};

} // namespace pipscope
