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

#ifdef __linux__
#include <poll.h>
#include <sys/socket.h>
#endif

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

// ── Pending frame accumulator ────────────────────────────────────────────────

struct PendingFrame {
    bool active = false;
    uint32_t expected_sequence = 0;
    uint64_t start_timestamp_ns = 0;
    uint64_t next_iteration = 0;
    uint8_t dtype = 0;
    double sample_rate_hz = 0.0;
    std::vector<float> samples;

    void reset() {
        active = false;
        samples.clear();
    }
};

// ── ChannelState (shared, protected by mutex_) ──────────────────────────────

struct ChannelState {
    uint16_t chan_id;
    double sample_rate_hz = 0.0;
    uint32_t last_sequence = 0;
    uint64_t packet_count = 0;
    SampleBuffer buffer;
    FrameStats stats;

    explicit ChannelState(uint16_t id, size_t buf_capacity) : chan_id(id), buffer(buf_capacity) {}
};

// ── ChannelRecvState (recv-thread-only, no lock needed) ─────────────────────

struct ChannelRecvState {
    PendingFrame pending;
    bool iter_tracking = false;
    uint64_t next_expected_iter = 0;
};

// ── PpktReceiver ─────────────────────────────────────────────────────────────

/// Snapshot of a single channel's state (thread-safe copy for rendering).
struct ChannelSnapshot {
    uint16_t chan_id;
    double sample_rate_hz;
    uint64_t packet_count;
    FrameStats stats;
    std::vector<float> samples;
};

/// Receiver-level metrics for observability (lock-free, atomic reads).
struct ReceiverMetrics {
    uint64_t recv_packets;
    uint64_t recv_bytes;
};

class PpktReceiver {
    int fd_ = -1;
    std::atomic<bool> running_{false};
    std::thread thread_;

    mutable std::mutex mutex_;
    std::map<uint16_t, ChannelState> channels_;
    size_t buffer_capacity_;

    // Lock-free recv metrics (incremented in recv_loop, read from GUI thread)
    std::atomic<uint64_t> recv_packets_{0};
    std::atomic<uint64_t> recv_bytes_{0};

    // Signal recv_loop to clear its local ChannelRecvState map (set by clear_channels)
    std::atomic<bool> recv_state_reset_{false};

  public:
    explicit PpktReceiver(size_t buffer_capacity = 1'000'000) : buffer_capacity_(buffer_capacity) {}

    ~PpktReceiver() { stop(); }

    size_t buffer_capacity() const { return buffer_capacity_; }

    /// Bind to UDP port on all interfaces and start the receiver thread.
    bool start(uint16_t port) {
        char addr_str[32];
        snprintf(addr_str, sizeof(addr_str), "0.0.0.0:%u", port);
        return start(addr_str);
    }

    /// Bind to the given address string and start the receiver thread.
    ///
    /// Preconditions: receiver must be stopped.
    /// Postconditions: on success, receiver thread is running.
    /// Failure modes: returns false if address is invalid or bind fails.
    /// Side effects: opens a socket and spawns a background thread.
    ///
    /// Supports "host:port" (UDP) and "unix:///path" (Unix domain socket).
    bool start(const char *address) {
        pipit::net::ParsedAddr pa = pipit::net::parse_address(address, std::strlen(address));
        if (pa.kind == pipit::net::AddrKind::INVALID)
            return false;

        int domain = (pa.kind == pipit::net::AddrKind::UNIX) ? AF_UNIX : AF_INET;
        fd_ = ::socket(domain, SOCK_DGRAM, 0);
        if (fd_ < 0)
            return false;

        if (domain == AF_INET) {
            int optval = 1;
            setsockopt(fd_, SOL_SOCKET, SO_REUSEADDR, &optval, sizeof(optval));
        }

        // Enlarge receive buffer to reduce kernel-level drops at high packet rates.
        // SO_RCVBUF is silently capped by /proc/sys/net/core/rmem_max (often 212 KB).
        // Try SO_RCVBUFFORCE first (requires CAP_NET_ADMIN), fall back to SO_RCVBUF.
        int rcvbuf = 16 * 1024 * 1024; // 16 MB requested
        if (setsockopt(fd_, SOL_SOCKET, SO_RCVBUFFORCE, &rcvbuf, sizeof(rcvbuf)) < 0) {
            setsockopt(fd_, SOL_SOCKET, SO_RCVBUF, &rcvbuf, sizeof(rcvbuf));
        }

        if (::bind(fd_, reinterpret_cast<const struct sockaddr *>(&pa.storage), pa.len) < 0) {
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
            snap.stats = ch.stats;
            snap.samples.resize(max_samples);
            size_t n = ch.buffer.snapshot(snap.samples.data(), max_samples);
            snap.samples.resize(n);
            result.push_back(std::move(snap));
        }

        return result;
    }

    /// Fill caller-owned snapshot vector, reusing existing allocations.
    /// After first frame, steady-state calls perform zero heap allocations.
    void snapshot_into(std::vector<ChannelSnapshot> &out, size_t max_samples) const {
        std::lock_guard<std::mutex> lock(mutex_);
        out.resize(channels_.size());
        size_t idx = 0;
        for (auto &[id, ch] : channels_) {
            auto &snap = out[idx++];
            snap.chan_id = ch.chan_id;
            snap.sample_rate_hz = ch.sample_rate_hz;
            snap.packet_count = ch.packet_count;
            snap.stats = ch.stats;
            snap.samples.resize(max_samples); // no-op when capacity already sufficient
            size_t n = ch.buffer.snapshot(snap.samples.data(), max_samples);
            snap.samples.resize(n); // shrink size, no dealloc
        }
    }

    /// Clear all channel data. Called on reconnect to discard stale data.
    void clear_channels() {
        std::lock_guard<std::mutex> lock(mutex_);
        channels_.clear();
        recv_state_reset_.store(true, std::memory_order_release);
    }

    /// Returns true if the receiver thread is currently running.
    bool is_running() const { return running_.load(); }

    /// Return lock-free receiver metrics (packet/byte counters).
    ReceiverMetrics metrics() const {
        return {recv_packets_.load(std::memory_order_relaxed),
                recv_bytes_.load(std::memory_order_relaxed)};
    }

    // Non-copyable
    PpktReceiver(const PpktReceiver &) = delete;
    PpktReceiver &operator=(const PpktReceiver &) = delete;

  private:
    static constexpr size_t kMaxPacketBytes = 65536;
    static constexpr size_t kMaxConvertedSamples = 8192;
    static constexpr auto kPollSleep = std::chrono::microseconds(10);

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

    enum class DropReason { SeqGap, IterGap, Boundary, MetaMismatch };

    /// Record a frame drop with reason. Called under mutex.
    static void record_drop(ChannelState &ch, ChannelRecvState &rs, DropReason reason) {
        ch.stats.dropped_frames++;
        switch (reason) {
        case DropReason::SeqGap:
            ch.stats.drop_seq_gap++;
            break;
        case DropReason::IterGap:
            ch.stats.drop_iter_gap++;
            break;
        case DropReason::Boundary:
            ch.stats.drop_boundary++;
            break;
        case DropReason::MetaMismatch:
            ch.stats.drop_meta_mismatch++;
            break;
        }
        rs.pending.reset();
    }

    /// Assemble a decoded packet into frames.
    /// recv_state is recv-thread-local — no lock needed for accumulation.
    /// Lock is only acquired for the brief commit to shared ChannelState.
    void assemble_frame(const pipit::net::PpktHeader &hdr, const float *samples,
                        size_t sample_count, ChannelRecvState &rs) {
        bool is_start = (hdr.flags & pipit::net::FLAG_FRAME_START) != 0;
        bool is_end = (hdr.flags & pipit::net::FLAG_FRAME_END) != 0;

        // ── Start of a new frame (lock-free accumulation) ──
        if (is_start) {
            if (rs.pending.active) {
                // Previous frame never closed — drop it (needs lock for stats)
                std::lock_guard<std::mutex> lock(mutex_);
                auto &ch = get_or_create_channel(hdr.chan_id);
                ch.packet_count++;
                record_drop(ch, rs, DropReason::Boundary);
            }

            // ── Inter-frame iteration_index continuity check ──
            if (hdr.flags & pipit::net::FLAG_FIRST_FRAME) {
                rs.iter_tracking = false;
            }
            bool has_gap = rs.iter_tracking && hdr.iteration_index != rs.next_expected_iter;

            rs.pending.active = true;
            rs.pending.expected_sequence = hdr.sequence + 1;
            rs.pending.start_timestamp_ns = hdr.timestamp_ns;
            rs.pending.next_iteration = hdr.iteration_index + sample_count;
            rs.pending.dtype = hdr.dtype;
            rs.pending.sample_rate_hz = hdr.sample_rate_hz;
            rs.pending.samples.assign(samples, samples + sample_count);

            if (is_end) {
                // Single-chunk frame — commit under lock
                std::lock_guard<std::mutex> lock(mutex_);
                auto &ch = get_or_create_channel(hdr.chan_id);
                ch.sample_rate_hz = hdr.sample_rate_hz;
                ch.last_sequence = hdr.sequence;
                ch.packet_count++;
                if (has_gap) {
                    ch.stats.inter_frame_gaps++;
                    ch.buffer.clear();
                }
                ch.stats.accepted_frames++;
                ch.buffer.push(rs.pending.samples.data(), rs.pending.samples.size());
                rs.pending.reset();
                rs.iter_tracking = true;
                rs.next_expected_iter = hdr.iteration_index + sample_count;
            } else {
                // Multi-chunk frame start — update metadata under lock
                std::lock_guard<std::mutex> lock(mutex_);
                auto &ch = get_or_create_channel(hdr.chan_id);
                ch.sample_rate_hz = hdr.sample_rate_hz;
                ch.last_sequence = hdr.sequence;
                ch.packet_count++;
                if (has_gap) {
                    ch.stats.inter_frame_gaps++;
                    ch.buffer.clear();
                }
            }
            return;
        }

        // ── Continuation / end chunk without a preceding start ──
        if (!rs.pending.active) {
            std::lock_guard<std::mutex> lock(mutex_);
            auto &ch = get_or_create_channel(hdr.chan_id);
            ch.packet_count++;
            record_drop(ch, rs, DropReason::Boundary);
            return;
        }

        // ── Validate sequence continuity (lock-free) ──
        if (hdr.sequence != rs.pending.expected_sequence) {
            std::lock_guard<std::mutex> lock(mutex_);
            auto &ch = get_or_create_channel(hdr.chan_id);
            ch.packet_count++;
            record_drop(ch, rs, DropReason::SeqGap);
            return;
        }

        // ── Validate iteration continuity (lock-free) ──
        if (hdr.iteration_index != rs.pending.next_iteration) {
            std::lock_guard<std::mutex> lock(mutex_);
            auto &ch = get_or_create_channel(hdr.chan_id);
            ch.packet_count++;
            record_drop(ch, rs, DropReason::IterGap);
            return;
        }

        // ── Validate metadata consistency within frame (lock-free) ──
        if (hdr.timestamp_ns != rs.pending.start_timestamp_ns || hdr.dtype != rs.pending.dtype ||
            hdr.sample_rate_hz != rs.pending.sample_rate_hz) {
            std::lock_guard<std::mutex> lock(mutex_);
            auto &ch = get_or_create_channel(hdr.chan_id);
            ch.packet_count++;
            record_drop(ch, rs, DropReason::MetaMismatch);
            return;
        }

        // ── Accumulate chunk (lock-free) ──
        rs.pending.samples.insert(rs.pending.samples.end(), samples, samples + sample_count);
        rs.pending.expected_sequence = hdr.sequence + 1;
        rs.pending.next_iteration = hdr.iteration_index + sample_count;

        // ── Commit if frame is complete (brief lock) ──
        if (is_end) {
            std::lock_guard<std::mutex> lock(mutex_);
            auto &ch = get_or_create_channel(hdr.chan_id);
            ch.sample_rate_hz = hdr.sample_rate_hz;
            ch.last_sequence = hdr.sequence;
            ch.packet_count++;
            ch.stats.accepted_frames++;
            ch.buffer.push(rs.pending.samples.data(), rs.pending.samples.size());
            rs.iter_tracking = true;
            rs.next_expected_iter = rs.pending.next_iteration;
            rs.pending.reset();
        } else {
            // Continuation chunk — update packet_count under lock
            std::lock_guard<std::mutex> lock(mutex_);
            auto &ch = get_or_create_channel(hdr.chan_id);
            ch.last_sequence = hdr.sequence;
            ch.packet_count++;
        }
    }

    /// Process a single raw packet: decode header+samples, then assemble frame.
    void process_packet(const uint8_t *buf, size_t packet_size, float *conv_buf,
                        size_t conv_capacity, std::map<uint16_t, ChannelRecvState> &recv_state) {
        recv_packets_.fetch_add(1, std::memory_order_relaxed);
        recv_bytes_.fetch_add(packet_size, std::memory_order_relaxed);

        pipit::net::PpktHeader hdr{};
        const uint8_t *payload = nullptr;
        size_t payload_bytes = 0;
        if (!decode_packet(buf, packet_size, hdr, payload, payload_bytes))
            return;

        size_t converted = 0;
        if (!decode_samples(payload, payload_bytes, hdr, conv_buf, conv_capacity, converted))
            return;

        auto &rs = recv_state[hdr.chan_id];
        assemble_frame(hdr, conv_buf, converted, rs);
    }

    void recv_loop() {
        std::array<float, kMaxConvertedSamples> conv_buf{};
        std::map<uint16_t, ChannelRecvState> recv_state;

#ifdef __linux__
        // ── Linux optimized path: poll() + recvmmsg() drain loop ──
        static constexpr int kBatchSize = 16;

        // Allocate batch buffers once
        std::vector<std::array<uint8_t, kMaxPacketBytes>> batch_bufs(kBatchSize);
        std::vector<struct iovec> iovecs(kBatchSize);
        std::vector<struct mmsghdr> msgs(kBatchSize);

        for (int i = 0; i < kBatchSize; ++i) {
            iovecs[i].iov_base = batch_bufs[i].data();
            iovecs[i].iov_len = kMaxPacketBytes;
            std::memset(&msgs[i], 0, sizeof(struct mmsghdr));
            msgs[i].msg_hdr.msg_iov = &iovecs[i];
            msgs[i].msg_hdr.msg_iovlen = 1;
        }

        struct pollfd pfd{};
        pfd.fd = fd_;
        pfd.events = POLLIN;

        while (running_.load()) {
            // Check if clear_channels() requested a recv_state reset
            if (recv_state_reset_.exchange(false, std::memory_order_acquire))
                recv_state.clear();

            int pr = ::poll(&pfd, 1, 1); // 1ms timeout for running_ flag check

            if (pr < 0) {
                if (errno == EINTR)
                    continue;
                break; // fatal (EBADF, ENOMEM)
            }
            if (pr == 0)
                continue; // timeout, check running_ flag

            // Drain all available packets
            for (;;) {
                int n = ::recvmmsg(fd_, msgs.data(), kBatchSize, MSG_DONTWAIT, nullptr);
                if (n > 0) {
                    for (int i = 0; i < n; ++i) {
                        process_packet(batch_bufs[i].data(), msgs[i].msg_len, conv_buf.data(),
                                       conv_buf.size(), recv_state);
                    }
                    // Reset iovecs for next batch (recvmmsg may modify iov_len)
                    for (int i = 0; i < n; ++i) {
                        iovecs[i].iov_len = kMaxPacketBytes;
                    }
                } else if (n < 0) {
                    if (errno == EINTR)
                        continue;
                    if (errno == EAGAIN || errno == EWOULDBLOCK)
                        break; // socket drained → back to poll
                    break;     // unexpected error → back to poll
                } else {
                    break; // n==0, shouldn't happen
                }
            }
        }
#else
        // ── Fallback path: single recvfrom + sleep ──
        alignas(8) std::array<uint8_t, kMaxPacketBytes> buf{};

        while (running_.load()) {
            // Check if clear_channels() requested a recv_state reset
            if (recv_state_reset_.exchange(false, std::memory_order_acquire))
                recv_state.clear();

            size_t packet_size = 0;
            RecvStatus status = recv_datagram(buf.data(), buf.size(), packet_size);
            if (status == RecvStatus::Retry)
                continue;
            if (status == RecvStatus::Fatal)
                break;

            process_packet(buf.data(), packet_size, conv_buf.data(), conv_buf.size(), recv_state);
        }
#endif
    }
};

} // namespace pipscope
