#pragma once
/// @file ppkt_receiver.h
/// @brief PPKT packet receiver with per-channel sample buffers for pipscope

#include <algorithm>
#include <atomic>
#include <cmath>
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

/// Convert PPKT payload of any dtype to float samples.
/// Returns number of float samples written to out.
inline size_t convert_to_float(const uint8_t *payload, uint32_t sample_count, uint8_t dtype,
                               float *out) {
    switch (static_cast<pipit::net::DType>(dtype)) {
    case pipit::net::DTYPE_F32:
        std::memcpy(out, payload, sample_count * sizeof(float));
        return sample_count;
    case pipit::net::DTYPE_I32:
        for (uint32_t i = 0; i < sample_count; i++) {
            int32_t v;
            std::memcpy(&v, payload + i * 4, 4);
            out[i] = static_cast<float>(v);
        }
        return sample_count;
    case pipit::net::DTYPE_CF32:
        // Extract magnitude from complex pairs
        for (uint32_t i = 0; i < sample_count; i++) {
            float re, im;
            std::memcpy(&re, payload + i * 8, 4);
            std::memcpy(&im, payload + i * 8 + 4, 4);
            out[i] = std::sqrt(re * re + im * im);
        }
        return sample_count;
    case pipit::net::DTYPE_F64:
        for (uint32_t i = 0; i < sample_count; i++) {
            double v;
            std::memcpy(&v, payload + i * 8, 8);
            out[i] = static_cast<float>(v);
        }
        return sample_count;
    case pipit::net::DTYPE_I16:
        for (uint32_t i = 0; i < sample_count; i++) {
            int16_t v;
            std::memcpy(&v, payload + i * 2, 2);
            out[i] = static_cast<float>(v);
        }
        return sample_count;
    case pipit::net::DTYPE_I8:
        for (uint32_t i = 0; i < sample_count; i++) {
            out[i] = static_cast<float>(static_cast<int8_t>(payload[i]));
        }
        return sample_count;
    default:
        return 0;
    }
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
    void recv_loop() {
        alignas(8) uint8_t buf[65536];
        // Temporary buffer for float conversion
        float conv_buf[8192];

        while (running_.load()) {
            ssize_t n = ::recvfrom(fd_, buf, sizeof(buf), 0, nullptr, nullptr);
            if (n <= 0) {
                if (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK) {
                    // Actual error — break
                    break;
                }
                // No data: sleep briefly to avoid busy-wait
                std::this_thread::sleep_for(std::chrono::microseconds(100));
                continue;
            }

            // Validate minimum size
            if (static_cast<size_t>(n) < sizeof(pipit::net::PpktHeader))
                continue;

            pipit::net::PpktHeader hdr;
            std::memcpy(&hdr, buf, sizeof(pipit::net::PpktHeader));

            if (!pipit::net::ppkt_validate(hdr))
                continue;

            // Validate payload size
            size_t payload_offset = sizeof(pipit::net::PpktHeader);
            size_t payload_avail = static_cast<size_t>(n) - payload_offset;
            if (payload_avail < hdr.payload_bytes)
                continue;

            // Convert payload to float
            uint32_t sample_count = hdr.sample_count;
            if (sample_count > 8192)
                sample_count = 8192; // clamp to conv_buf size
            size_t converted =
                convert_to_float(buf + payload_offset, sample_count, hdr.dtype, conv_buf);
            if (converted == 0)
                continue;

            // Push into channel buffer
            std::lock_guard<std::mutex> lock(mutex_);
            auto it = channels_.find(hdr.chan_id);
            if (it == channels_.end()) {
                auto [ins, _] =
                    channels_.emplace(hdr.chan_id, ChannelState(hdr.chan_id, buffer_capacity_));
                it = ins;
            }

            auto &ch = it->second;
            ch.sample_rate_hz = hdr.sample_rate_hz;
            ch.last_sequence = hdr.sequence;
            ch.packet_count++;
            ch.buffer.push(conv_buf, converted);
        }
    }
};

} // namespace pipscope
