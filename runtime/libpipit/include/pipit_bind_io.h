#pragma once
/// @file pipit_bind_io.h
/// @brief Bind I/O adapter — automatic PPKT send/recv for bind-backed buffers
///
/// Provides BindIoAdapter, which wraps DatagramSender/DatagramReceiver with
/// lazy initialization, reconnect support, and thread-safe I/O for bind
/// endpoints.  Generated code creates one adapter per lowered bind.
///
/// See doc/spec/pcc-spec-v0.4.0.md §5.5 for the bind compilation contract.

#include <pipit.h>
#include <pipit_net.h>
#include <pipit_shell.h>

#include <cstdio>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>

namespace pipit {

// ── Address extraction ──────────────────────────────────────────────────────

/// Extract the raw address from a spec-style or raw endpoint string.
///
/// Handles all input sources:
///   - Spec string: `udp("127.0.0.1:9100", chan=10)` → `127.0.0.1:9100`
///   - Raw address: `127.0.0.1:9100` → `127.0.0.1:9100`
///   - Empty: `""` → `""`
///
/// Free function (not a class member) for direct testability.
inline std::string extract_address(const std::string &ep) {
    auto q1 = ep.find('"');
    if (q1 != std::string::npos) {
        auto q2 = ep.find('"', q1 + 1);
        if (q2 != std::string::npos)
            return ep.substr(q1 + 1, q2 - q1 - 1);
    }
    return ep; // raw address or empty
}

// ── Bind I/O adapter ────────────────────────────────────────────────────────

class BindIoAdapter {
    const char *name_;
    pipit::net::DType dtype_;
    uint16_t chan_id_;
    double rate_hz_;
    bool is_out_;
    std::string transport_;
    BindState *state_;

    std::unique_ptr<pipit::net::DatagramSender> sender_;
    std::unique_ptr<pipit::net::DatagramReceiver> receiver_;
    pipit::net::PpktHeader hdr_;
    bool initialized_ = false;
    int init_fail_count_ = 0;
    static constexpr int MAX_INIT_RETRIES = 3;
    std::string endpoint_;
    std::mutex io_mtx_;
    uint8_t recv_buf_[65536];

  public:
    BindIoAdapter(const char *name, bool is_out, pipit::net::DType dtype, uint16_t chan_id,
                  double rate_hz, const char *transport, BindState *state)
        : name_(name), dtype_(dtype), chan_id_(chan_id), rate_hz_(rate_hz), is_out_(is_out),
          transport_(transport), state_(state) {
        hdr_ = pipit::net::ppkt_make_header(dtype, chan_id);
        hdr_.flags = pipit::net::FLAG_FIRST_FRAME;
        std::memset(recv_buf_, 0, sizeof(recv_buf_));
    }

    /// Send data to the bind endpoint via PPKT.
    void send(const void *data, uint32_t n_tokens) {
        std::lock_guard<std::mutex> lk(io_mtx_);
        if (!initialized_)
            lazy_init();
        if (!sender_ || !sender_->is_valid())
            return;

        hdr_.sample_count = n_tokens;
        hdr_.payload_bytes = static_cast<uint32_t>(n_tokens * pipit::net::dtype_size(dtype_));
        hdr_.sample_rate_hz = pipit_task_rate_hz();
        hdr_.timestamp_ns = pipit_now_ns();
        hdr_.iteration_index = pipit_iteration_index();

        pipit::net::ppkt_send_chunked(*sender_, hdr_, data, n_tokens);
        hdr_.sequence++;
        hdr_.flags &= ~pipit::net::FLAG_FIRST_FRAME;
    }

    /// Receive data from the bind endpoint via PPKT.
    /// Zero-fills output if no valid data is available.
    void recv(void *out, uint32_t n_tokens) {
        std::lock_guard<std::mutex> lk(io_mtx_);
        size_t fill_bytes = n_tokens * pipit::net::dtype_size(dtype_);
        std::memset(out, 0, fill_bytes);

        if (!initialized_)
            lazy_init();
        if (!receiver_ || !receiver_->is_valid())
            return;

        // Drain all available packets, keep latest valid one
        ssize_t latest_len = 0;
        for (;;) {
            ssize_t r = receiver_->recv(recv_buf_, sizeof(recv_buf_));
            if (r <= 0)
                break;
            latest_len = r;
        }

        if (latest_len < static_cast<ssize_t>(sizeof(pipit::net::PpktHeader)))
            return;

        const auto *pkt_hdr = reinterpret_cast<const pipit::net::PpktHeader *>(recv_buf_);
        if (!pipit::net::ppkt_validate(*pkt_hdr))
            return;
        if (pkt_hdr->dtype != static_cast<uint8_t>(dtype_))
            return;

        size_t header_size = sizeof(pipit::net::PpktHeader);
        size_t available_bytes = std::min(static_cast<size_t>(pkt_hdr->payload_bytes),
                                          static_cast<size_t>(latest_len) - header_size);
        size_t copy_bytes = std::min(available_bytes, fill_bytes);
        std::memcpy(out, recv_buf_ + header_size, copy_bytes);
    }

    /// Reconnect to a new endpoint. Called after rebind.
    /// Empty string disconnects (next I/O becomes no-op).
    void reconnect(const std::string &new_endpoint) {
        std::lock_guard<std::mutex> lk(io_mtx_);
        sender_.reset();
        receiver_.reset();
        endpoint_ = resolve_address(extract_address(new_endpoint));
        initialized_ = false;
        init_fail_count_ = 0;
    }

  private:
    void lazy_init() {
        // Already holding io_mtx_
        if (init_fail_count_ >= MAX_INIT_RETRIES)
            return; // permanent no-op, already warned

        // Read current endpoint from BindState
        std::string ep;
        {
            std::lock_guard<std::mutex> lk(state_->mtx);
            ep = state_->current_endpoint;
        }
        endpoint_ = resolve_address(extract_address(ep));

        if (endpoint_.empty()) {
            initialized_ = true; // intentional no-op mode
            return;
        }

        bool ok = false;
        if (is_out_) {
            sender_ = std::make_unique<pipit::net::DatagramSender>();
            ok = sender_->open(endpoint_.c_str(), endpoint_.size());
        } else {
            receiver_ = std::make_unique<pipit::net::DatagramReceiver>();
            ok = receiver_->open(endpoint_.c_str(), endpoint_.size());
        }

        if (ok) {
            initialized_ = true;
        } else {
            init_fail_count_++;
            std::fprintf(stderr, "bind '%s': failed to open endpoint '%s' (attempt %d/%d)\n", name_,
                         endpoint_.c_str(), init_fail_count_, MAX_INIT_RETRIES);
            if (init_fail_count_ >= MAX_INIT_RETRIES) {
                std::fprintf(stderr, "bind '%s': giving up after %d attempts\n", name_,
                             MAX_INIT_RETRIES);
                initialized_ = true; // permanent no-op
            }
        }
    }

    std::string resolve_address(const std::string &raw_addr) {
        if (transport_ == "unix_dgram" && raw_addr.substr(0, 7) != "unix://") {
            return "unix://" + raw_addr;
        }
        return raw_addr;
    }
};

} // namespace pipit
