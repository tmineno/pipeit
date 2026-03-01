#pragma once
/// @file ppkt_receiver.h
/// @brief PPKT packet receiver with per-channel sample buffers for pipscope

#include <array>
#include <atomic>
#include <chrono>
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

#include "types.h"

namespace pipscope {

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
