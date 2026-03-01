#pragma once
/// @file shm_receiver.h
/// @brief PSHM shared memory reader for pipscope monitoring
///
/// Provides ShmReceiver: a monitoring-mode SHM reader that auto-discovers
/// parameters from the Superblock and continuously polls for new data.
///
/// Preconditions: The named SHM object must exist and contain a valid PSHM Superblock.
/// Postconditions: On successful start(), a poll thread reads new slots and pushes
///   float samples into a SampleBuffer.
/// Failure modes: start() returns false if SHM cannot be opened or Superblock is invalid.
/// Side effects: Opens a POSIX shared memory object and spawns a background thread.

#include <atomic>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include <pipit_net.h>
#include <pipit_shm.h>

#include "types.h"

#if defined(__unix__)
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

namespace pipscope {

// ── Channel ID helper ────────────────────────────────────────────────────────

/// Deterministic channel ID from SHM name (FNV-1a hash mapped to 0x8001–0xFFFF).
/// Stable across runs for the same name.
inline uint16_t shm_chan_id(const char *name, uint16_t salt = 0) {
    uint64_t h = 14695981039346656037ULL; // FNV-1a basis
    for (const char *p = name; *p; ++p) {
        h ^= static_cast<uint64_t>(static_cast<uint8_t>(*p));
        h *= 1099511628211ULL;
    }
    h ^= static_cast<uint64_t>(salt);
    h *= 1099511628211ULL;
    return static_cast<uint16_t>((h % 0x7FFEu) + 0x8001u);
}

// ── SHM probe ────────────────────────────────────────────────────────────────

/// Discovered metadata from a PSHM Superblock.
struct ShmInfo {
    uint8_t dtype = 0;
    uint8_t rank = 0;
    uint32_t dims[8] = {};
    uint32_t slot_count = 0;
    uint32_t slot_payload_bytes = 0;
    uint32_t tokens_per_frame = 0;
    double rate_hz = 0.0;
    size_t total_size = 0;
    bool valid = false;
};

/// Probe a PSHM shared memory object and read its Superblock.
///
/// Opens the SHM object read-only, reads the 128-byte Superblock,
/// validates magic/version/header_len/geometry, and returns discovered metadata.
///
/// Preconditions: name must be a valid POSIX SHM name (with or without leading /).
/// Postconditions: On success, info.valid == true and info.total_size is computed.
/// Failure modes: Returns info with valid == false if SHM doesn't exist or is invalid.
/// Side effects: Opens and closes a file descriptor; no persistent state.
inline ShmInfo probe_shm(const char *name) {
    ShmInfo info;

#if !defined(__unix__)
    (void)name;
    return info;
#else
    // 1. Normalize name
    std::string norm;
    if (name == nullptr || name[0] == '\0') {
        return info;
    }
    if (name[0] == '/') {
        norm = name;
    } else {
        norm = std::string("/") + name;
    }

    // 2. Open read-only
    int fd = shm_open(norm.c_str(), O_RDONLY, 0);
    if (fd < 0) {
        return info;
    }

    // 3. fstat to get actual file size
    struct stat st{};
    if (fstat(fd, &st) < 0) {
        ::close(fd);
        return info;
    }
    if (st.st_size < static_cast<off_t>(sizeof(pipit::shm::Superblock))) {
        ::close(fd);
        return info;
    }

    // 4. mmap just the Superblock (128 bytes)
    void *addr = mmap(nullptr, sizeof(pipit::shm::Superblock), PROT_READ, MAP_SHARED, fd, 0);
    ::close(fd);
    if (addr == MAP_FAILED) {
        return info;
    }

    const auto *sb = reinterpret_cast<const pipit::shm::Superblock *>(addr);

    // 5. Validate magic, version, header_len
    if (std::memcmp(sb->magic, pipit::shm::PSHM_MAGIC, 4) != 0 ||
        sb->version != pipit::shm::PSHM_VERSION || sb->header_len != pipit::shm::PSHM_HEADER_LEN) {
        munmap(addr, sizeof(pipit::shm::Superblock));
        return info;
    }

    // 6. Validate geometry
    if (sb->slot_count == 0 || sb->slot_payload_bytes % 8 != 0) {
        munmap(addr, sizeof(pipit::shm::Superblock));
        return info;
    }

    // 7. Overflow-safe total_size computation
    size_t slot_stride = sizeof(pipit::shm::SlotHeader) + sb->slot_payload_bytes;
    // Check for multiplication overflow
    if (slot_stride > 0 &&
        sb->slot_count > (SIZE_MAX - sizeof(pipit::shm::Superblock)) / slot_stride) {
        munmap(addr, sizeof(pipit::shm::Superblock));
        return info;
    }
    size_t total_size = sizeof(pipit::shm::Superblock) + sb->slot_count * slot_stride;

    // 8. Validate file size >= total_size
    if (static_cast<size_t>(st.st_size) < total_size) {
        munmap(addr, sizeof(pipit::shm::Superblock));
        return info;
    }

    // 9. Extract metadata
    info.dtype = sb->dtype;
    info.rank = sb->rank;
    std::memcpy(info.dims, sb->dims, sizeof(info.dims));
    info.slot_count = sb->slot_count;
    info.slot_payload_bytes = sb->slot_payload_bytes;
    info.tokens_per_frame = sb->tokens_per_frame;
    info.rate_hz = sb->rate_hz;
    info.total_size = total_size;
    info.valid = true;

    // 10. Cleanup
    munmap(addr, sizeof(pipit::shm::Superblock));
    return info;
#endif
}

// ── ShmReceiver ──────────────────────────────────────────────────────────────

/// Single-channel SHM receiver for pipscope monitoring.
///
/// Each instance monitors one PSHM ring. Spawns a poll thread that
/// reads new slots via ShmReader::consume(), converts samples to float,
/// and pushes them into a SampleBuffer accessible via snapshot_into().
class ShmReceiver {
    std::string name_;
    uint16_t chan_id_;
    std::string label_;
    size_t buffer_capacity_;

    pipit::shm::ShmReader reader_;
    ShmInfo info_;

    mutable std::mutex mutex_;
    SampleBuffer buffer_;
    double sample_rate_hz_ = 0.0;
    uint64_t slot_count_ = 0;
    FrameStats stats_;

    std::atomic<bool> running_{false};
    std::thread thread_;

    // Lock-free metrics (incremented in poll thread, read from GUI thread)
    std::atomic<uint64_t> recv_slots_{0};
    std::atomic<uint64_t> recv_bytes_{0};

  public:
    explicit ShmReceiver(const char *name, uint16_t chan_id, size_t buffer_capacity = 1'000'000)
        : name_(name), chan_id_(chan_id), label_(std::string("shm:") + name),
          buffer_capacity_(buffer_capacity), buffer_(buffer_capacity) {}

    ~ShmReceiver() { stop(); }

    /// Probe the SHM, attach via ShmReader, start the poll thread.
    ///
    /// Preconditions: Receiver must be stopped. SHM object must exist.
    /// Postconditions: On success, poll thread is running.
    /// Failure modes: Returns false if probe fails or attach fails.
    /// Side effects: Opens SHM (via ShmReader), spawns a thread.
    bool start() {
        info_ = probe_shm(name_.c_str());
        if (!info_.valid) {
            std::fprintf(stderr, "pipscope shm: failed to probe '%s'\n", name_.c_str());
            return false;
        }

        // Attach via ShmReader with discovered parameters.
        // Passes stable_id_hash=0 to skip that check (pipit_shm.h line 452).
        if (!reader_.attach(name_.c_str(), info_.slot_count, info_.slot_payload_bytes,
                            static_cast<pipit::net::DType>(info_.dtype), info_.rank, info_.dims,
                            info_.rate_hz, /*stable_id_hash=*/0)) {
            std::fprintf(stderr, "pipscope shm: failed to attach '%s'\n", name_.c_str());
            return false;
        }

        sample_rate_hz_ = info_.rate_hz;
        running_.store(true);
        thread_ = std::thread(&ShmReceiver::poll_loop, this);
        return true;
    }

    /// Stop the poll thread and close the SHM reader.
    void stop() {
        running_.store(false);
        if (thread_.joinable())
            thread_.join();
        reader_.close();
    }

    /// Fill one ChannelSnapshot for this SHM source.
    /// Thread-safe: called from GUI thread while poll thread runs.
    void snapshot_into(ChannelSnapshot &out, size_t max_samples) const {
        std::lock_guard<std::mutex> lock(mutex_);
        out.chan_id = chan_id_;
        out.sample_rate_hz = sample_rate_hz_;
        out.packet_count = slot_count_;
        out.stats = stats_;
        out.label = label_;
        out.samples.resize(max_samples);
        size_t n = buffer_.snapshot(out.samples.data(), max_samples);
        out.samples.resize(n);
    }

    /// Return receiver metrics (slot/byte counters).
    ReceiverMetrics metrics() const {
        return {recv_slots_.load(std::memory_order_relaxed),
                recv_bytes_.load(std::memory_order_relaxed)};
    }

    /// Clear all buffered data.
    void clear() {
        std::lock_guard<std::mutex> lock(mutex_);
        buffer_.clear();
        stats_ = {};
        slot_count_ = 0;
    }

    const std::string &name() const { return name_; }
    uint16_t chan_id() const { return chan_id_; }
    bool is_running() const { return running_.load(); }
    size_t buffer_capacity() const { return buffer_capacity_; }

    // Non-copyable
    ShmReceiver(const ShmReceiver &) = delete;
    ShmReceiver &operator=(const ShmReceiver &) = delete;

  private:
    static constexpr auto kPollSleep = std::chrono::microseconds(10);

    void poll_loop() {
        // Pre-allocate buffers for consume + conversion
        std::vector<uint8_t> raw_buf(info_.slot_payload_bytes);
        // Max float samples per slot: slot_payload_bytes / min_dtype_size(1 byte for i8)
        size_t max_float_samples = info_.slot_payload_bytes;
        std::vector<float> conv_buf(max_float_samples);

        size_t sample_bytes = dtype_sample_bytes(info_.dtype);

        while (running_.load()) {
            size_t bytes = reader_.consume(raw_buf.data(), raw_buf.size());
            if (bytes == 0) {
                std::this_thread::sleep_for(kPollSleep);
                continue;
            }

            recv_slots_.fetch_add(1, std::memory_order_relaxed);
            recv_bytes_.fetch_add(bytes, std::memory_order_relaxed);

            // Convert raw bytes to float samples
            uint32_t sample_count =
                (sample_bytes > 0) ? static_cast<uint32_t>(bytes / sample_bytes) : 0;
            if (sample_count == 0)
                continue;

            size_t float_count =
                convert_to_float(raw_buf.data(), bytes, sample_count, info_.dtype, conv_buf.data());

            if (float_count > 0) {
                std::lock_guard<std::mutex> lock(mutex_);
                buffer_.push(conv_buf.data(), float_count);
                stats_.accepted_frames++;
                slot_count_++;
            }
        }
    }
};

} // namespace pipscope
