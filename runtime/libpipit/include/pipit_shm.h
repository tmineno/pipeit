#pragma once
/// @file pipit_shm.h
/// @brief PSHM — Pipit Shared Memory Bind Transport
///
/// Implements the PSHM protocol (doc/spec/pshm-protocol-spec-v0.1.0.md) for
/// low-latency, non-blocking shared-memory IPC between Pipit processes on
/// the same host.
///
/// Provides:
///   - Superblock / SlotHeader binary layout (128B / 64B, packed POD)
///   - ShmRegion: POSIX shm lifecycle (create/open/map/unmap/close)
///   - ShmWriter: single-writer publish path (release-store)
///   - ShmReader: multi-reader consume path (acquire-load, overwrite detection)
///   - ShmIoAdapter: high-level adapter for generated code (lazy init, rebind)
///
/// Design decisions:
///   - All struct fields are plain POD (no std::atomic).  Logically-atomic
///     fields use __atomic_load_n/__atomic_store_n builtins, which are
///     cross-process safe when backed by lock-free hardware atomics.
///   - slot_bytes must be a multiple of 8 (enforced at compile time by E0726
///     and at runtime by init()/attach()).
///   - Platform: POSIX only (#if defined(__unix__)).  Non-unix targets get a
///     stub ShmIoAdapter that logs a fatal error and enters permanent no-op.

#include <pipit_net.h>
#include <pipit_shell.h>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>

#if defined(__unix__)
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

namespace pipit {
namespace shm {

// ── Constants ────────────────────────────────────────────────────────────────

static constexpr uint8_t PSHM_MAGIC[4] = {'P', 'S', 'H', 'M'};
static constexpr uint8_t PSHM_VERSION = 1;
static constexpr uint8_t PSHM_HEADER_LEN = 128;

// SlotHeader flags
static constexpr uint32_t FLAG_FRAME_START = 1u << 0;
static constexpr uint32_t FLAG_FRAME_END = 1u << 1;
static constexpr uint32_t FLAG_EPOCH_FENCE = 1u << 2;

// ── Lock-free guarantee ──────────────────────────────────────────────────────
//
// PSHM requires lock-free atomics for cross-process correctness (process-local
// mutexes are not shared across mmap).  We verify at compile time using
// alignas'd probe objects for reliable aligned-pointer checking.

namespace detail {
alignas(uint64_t) inline constexpr uint64_t lock_free_probe_u64 = 0;
alignas(uint32_t) inline constexpr uint32_t lock_free_probe_u32 = 0;
} // namespace detail

static_assert(__atomic_always_lock_free(sizeof(uint64_t), &detail::lock_free_probe_u64),
              "PSHM requires lock-free 64-bit atomics for cross-process correctness");
static_assert(__atomic_always_lock_free(sizeof(uint32_t), &detail::lock_free_probe_u32),
              "PSHM requires lock-free 32-bit atomics for cross-process correctness");

// ── Atomic helpers ───────────────────────────────────────────────────────────

inline uint64_t shm_load_acquire(const uint64_t *p) { return __atomic_load_n(p, __ATOMIC_ACQUIRE); }
inline void shm_store_release(uint64_t *p, uint64_t v) { __atomic_store_n(p, v, __ATOMIC_RELEASE); }
inline uint32_t shm_load_acquire(const uint32_t *p) { return __atomic_load_n(p, __ATOMIC_ACQUIRE); }
inline void shm_store_release(uint32_t *p, uint32_t v) { __atomic_store_n(p, v, __ATOMIC_RELEASE); }

// ── Binary layout structs ────────────────────────────────────────────────────

#pragma pack(push, 1)

/// Superblock — first 128 bytes of the shared memory object.
struct Superblock {
    uint8_t magic[4];             //  0: "PSHM"
    uint8_t version;              //  4: protocol version (1)
    uint8_t header_len;           //  5: superblock size (128)
    uint16_t flags;               //  6: reserved (0)
    uint8_t dtype;                //  8: sample type code
    uint8_t rank;                 //  9: shape rank (0..8)
    uint16_t reserved0;           // 10: must be 0
    uint32_t tokens_per_frame;    // 12: logical tokens per firing
    uint32_t slot_count;          // 16: ring slot count
    uint32_t slot_payload_bytes;  // 20: bytes per slot payload
    double rate_hz;               // 24: contract rate (tokens/sec)
    uint64_t stable_id_hash;      // 32: hash of compiler stable_id
    uint32_t epoch;               // 40: rebind generation (atomic)
    uint32_t reserved1;           // 44: must be 0
    uint64_t write_seq;           // 48: latest committed sequence (atomic)
    uint64_t writer_heartbeat_ns; // 56: monotonic heartbeat
    uint32_t dims[8];             // 64: shape dims; unused = 0
    uint64_t endpoint_name_hash;  // 96: hash of shm(name)
    uint8_t reserved2[24];        // 104: must be 0
};

/// SlotHeader — 64-byte header preceding each slot's payload.
struct SlotHeader {
    uint64_t seq;             //  0: committed sequence number (atomic)
    uint32_t epoch;           //  8: generation of this slot
    uint32_t flags;           // 12: bitfield (FLAG_FRAME_START/END/EPOCH_FENCE)
    uint64_t iteration_index; // 16: logical iteration index
    uint64_t timestamp_ns;    // 24: monotonic timestamp
    uint32_t token_count;     // 32: tokens in payload
    uint32_t payload_bytes;   // 36: bytes in payload
    uint8_t reserved[24];     // 40: must be 0
};

#pragma pack(pop)

// Layout verification
static_assert(sizeof(Superblock) == 128, "Superblock must be exactly 128 bytes");
static_assert(sizeof(SlotHeader) == 64, "SlotHeader must be exactly 64 bytes");
static_assert(offsetof(Superblock, write_seq) == 48, "write_seq must be at offset 48");
static_assert(offsetof(Superblock, epoch) == 40, "epoch must be at offset 40");
static_assert(offsetof(Superblock, endpoint_name_hash) == 96, "endpoint_name_hash at offset 96");
static_assert(offsetof(SlotHeader, seq) == 0, "slot seq must be at offset 0");

// ── Endpoint name hashing ────────────────────────────────────────────────────

/// FNV-1a 64-bit hash of the endpoint name for Superblock.endpoint_name_hash.
inline uint64_t hash_endpoint_name(const char *name) {
    uint64_t h = 14695981039346656037ULL; // FNV-1a basis
    for (const char *p = name; *p; ++p) {
        h ^= static_cast<uint64_t>(static_cast<uint8_t>(*p));
        h *= 1099511628211ULL; // FNV-1a prime
    }
    return h;
}

// ── Platform-gated implementation ────────────────────────────────────────────

#if defined(__unix__)

// ── ShmRegion — POSIX shm lifecycle ──────────────────────────────────────────

class ShmRegion {
    int fd_ = -1;
    void *addr_ = nullptr;
    size_t size_ = 0;
    std::string normalized_name_;
    bool owner_ = false; // true = writer created this region

  public:
    ShmRegion() = default;
    ~ShmRegion() { close(); }

    // Non-copyable
    ShmRegion(const ShmRegion &) = delete;
    ShmRegion &operator=(const ShmRegion &) = delete;

    /// Create a new shared memory object (writer).
    bool create(const char *name, size_t total_size) {
        normalized_name_ = normalize_name(name);
        // Remove stale object if present
        shm_unlink(normalized_name_.c_str());
        fd_ = shm_open(normalized_name_.c_str(), O_CREAT | O_RDWR, 0600);
        if (fd_ < 0) {
            std::fprintf(stderr, "pshm: shm_open create '%s' failed: %s\n",
                         normalized_name_.c_str(), std::strerror(errno));
            return false;
        }
        if (ftruncate(fd_, static_cast<off_t>(total_size)) < 0) {
            std::fprintf(stderr, "pshm: ftruncate '%s' to %zu failed: %s\n",
                         normalized_name_.c_str(), total_size, std::strerror(errno));
            ::close(fd_);
            fd_ = -1;
            shm_unlink(normalized_name_.c_str());
            return false;
        }
        addr_ = mmap(nullptr, total_size, PROT_READ | PROT_WRITE, MAP_SHARED, fd_, 0);
        if (addr_ == MAP_FAILED) {
            std::fprintf(stderr, "pshm: mmap '%s' failed: %s\n", normalized_name_.c_str(),
                         std::strerror(errno));
            addr_ = nullptr;
            ::close(fd_);
            fd_ = -1;
            shm_unlink(normalized_name_.c_str());
            return false;
        }
        std::memset(addr_, 0, total_size);
        size_ = total_size;
        owner_ = true;
        return true;
    }

    /// Open an existing shared memory object (reader).
    bool open(const char *name, size_t total_size) {
        normalized_name_ = normalize_name(name);
        fd_ = shm_open(normalized_name_.c_str(), O_RDWR, 0);
        if (fd_ < 0) {
            std::fprintf(stderr, "pshm: shm_open read '%s' failed: %s\n", normalized_name_.c_str(),
                         std::strerror(errno));
            return false;
        }
        addr_ = mmap(nullptr, total_size, PROT_READ | PROT_WRITE, MAP_SHARED, fd_, 0);
        if (addr_ == MAP_FAILED) {
            std::fprintf(stderr, "pshm: mmap read '%s' failed: %s\n", normalized_name_.c_str(),
                         std::strerror(errno));
            addr_ = nullptr;
            ::close(fd_);
            fd_ = -1;
            return false;
        }
        size_ = total_size;
        owner_ = false;
        return true;
    }

    void close() {
        if (addr_) {
            munmap(addr_, size_);
            addr_ = nullptr;
        }
        if (fd_ >= 0) {
            ::close(fd_);
            fd_ = -1;
        }
        if (owner_ && !normalized_name_.empty()) {
            shm_unlink(normalized_name_.c_str());
            owner_ = false;
        }
        normalized_name_.clear();
        size_ = 0;
    }

    void *data() { return addr_; }
    const void *data() const { return addr_; }
    size_t size() const { return size_; }
    bool is_mapped() const { return addr_ != nullptr; }

  private:
    /// Normalize POSIX shm name: prepend '/' if absent.
    static std::string normalize_name(const char *name) {
        if (!name || name[0] == '\0')
            return "/pshm_default";
        if (name[0] == '/')
            return std::string(name);
        return std::string("/") + name;
    }
};

// ── ShmWriter — single-writer publish path ───────────────────────────────────

class ShmWriter {
    ShmRegion region_;
    uint32_t slot_count_ = 0;
    uint32_t slot_payload_bytes_ = 0;
    uint64_t next_seq_ = 1; // sequences start at 1 (0 = uninitialized)
    uint32_t current_epoch_ = 0;
    bool valid_ = false;

  public:
    ShmWriter() = default;

    /// Initialize the writer: create shm, write superblock.
    bool init(const char *name, uint32_t slot_count, uint32_t slot_payload_bytes,
              pipit::net::DType dtype, uint8_t rank, const uint32_t *dims,
              uint32_t tokens_per_frame, double rate_hz, uint64_t stable_id_hash) {
        if (slot_payload_bytes % 8 != 0) {
            std::fprintf(stderr, "pshm writer: slot_payload_bytes=%u is not 8-byte aligned\n",
                         slot_payload_bytes);
            return false;
        }

        size_t slot_stride = sizeof(SlotHeader) + slot_payload_bytes;
        size_t total_size = sizeof(Superblock) + slot_count * slot_stride;

        if (!region_.create(name, total_size))
            return false;

        // Write superblock
        auto *sb = reinterpret_cast<Superblock *>(region_.data());
        std::memcpy(sb->magic, PSHM_MAGIC, 4);
        sb->version = PSHM_VERSION;
        sb->header_len = PSHM_HEADER_LEN;
        sb->flags = 0;
        sb->dtype = static_cast<uint8_t>(dtype);
        sb->rank = rank;
        sb->reserved0 = 0;
        sb->tokens_per_frame = tokens_per_frame;
        sb->slot_count = slot_count;
        sb->slot_payload_bytes = slot_payload_bytes;
        sb->rate_hz = rate_hz;
        sb->stable_id_hash = stable_id_hash;
        sb->epoch = 0;
        sb->reserved1 = 0;
        sb->write_seq = 0;
        sb->writer_heartbeat_ns = pipit_now_ns();
        for (uint8_t i = 0; i < 8; ++i)
            sb->dims[i] = (dims && i < rank) ? dims[i] : 0;
        sb->endpoint_name_hash = hash_endpoint_name(name);
        std::memset(sb->reserved2, 0, sizeof(sb->reserved2));

        slot_count_ = slot_count;
        slot_payload_bytes_ = slot_payload_bytes;
        next_seq_ = 1;
        current_epoch_ = 0;
        valid_ = true;
        return true;
    }

    /// Publish a data frame to the ring.
    bool publish(const void *data, uint32_t payload_bytes, uint32_t token_count, uint32_t flags,
                 uint64_t iteration_index) {
        if (!valid_)
            return false;
        if (payload_bytes > slot_payload_bytes_)
            return false;

        auto *sb = superblock();
        size_t slot_stride = sizeof(SlotHeader) + slot_payload_bytes_;
        uint32_t idx = static_cast<uint32_t>(next_seq_ % slot_count_);

        auto *slot = reinterpret_cast<SlotHeader *>(static_cast<uint8_t *>(region_.data()) +
                                                    sizeof(Superblock) + idx * slot_stride);
        uint8_t *payload_ptr = reinterpret_cast<uint8_t *>(slot) + sizeof(SlotHeader);

        // Write payload
        std::memcpy(payload_ptr, data, payload_bytes);

        // Write slot metadata
        slot->epoch = current_epoch_;
        slot->flags = flags;
        slot->iteration_index = iteration_index;
        slot->timestamp_ns = pipit_now_ns();
        slot->token_count = token_count;
        slot->payload_bytes = payload_bytes;
        std::memset(slot->reserved, 0, sizeof(slot->reserved));

        // Publish: store_release(slot.seq), then store_release(write_seq)
        shm_store_release(&slot->seq, next_seq_);
        shm_store_release(&sb->write_seq, next_seq_);
        sb->writer_heartbeat_ns = pipit_now_ns();

        next_seq_++;
        return true;
    }

    /// Emit an epoch fence slot and increment the epoch.
    void emit_epoch_fence(uint64_t iteration_index) {
        if (!valid_)
            return;
        publish(nullptr, 0, 0, FLAG_EPOCH_FENCE, iteration_index);
        current_epoch_++;
        auto *sb = superblock();
        shm_store_release(&sb->epoch, current_epoch_);
    }

    void close() {
        region_.close();
        valid_ = false;
    }

    bool is_valid() const { return valid_; }

  private:
    Superblock *superblock() { return reinterpret_cast<Superblock *>(region_.data()); }
};

// ── ShmReader — multi-reader consume path ────────────────────────────────────

class ShmReader {
    ShmRegion region_;
    uint32_t slot_count_ = 0;
    uint32_t slot_payload_bytes_ = 0;
    uint64_t want_seq_ = 0;
    uint32_t known_epoch_ = 0;
    bool valid_ = false;

  public:
    ShmReader() = default;

    /// Attach to an existing shm object and validate the contract.
    bool attach(const char *name, uint32_t expected_slot_count, uint32_t expected_slot_bytes,
                pipit::net::DType expected_dtype, uint8_t expected_rank,
                const uint32_t *expected_dims, double expected_rate_hz,
                uint64_t expected_stable_id_hash) {
        if (expected_slot_bytes % 8 != 0) {
            std::fprintf(stderr, "pshm reader: expected_slot_bytes=%u is not 8-byte aligned\n",
                         expected_slot_bytes);
            return false;
        }

        size_t slot_stride = sizeof(SlotHeader) + expected_slot_bytes;
        size_t total_size = sizeof(Superblock) + expected_slot_count * slot_stride;

        if (!region_.open(name, total_size))
            return false;

        const auto *sb = superblock();

        // Validate magic
        if (std::memcmp(sb->magic, PSHM_MAGIC, 4) != 0) {
            std::fprintf(stderr, "pshm reader: invalid magic in '%s'\n", name);
            region_.close();
            return false;
        }

        // Validate version
        if (sb->version != PSHM_VERSION) {
            std::fprintf(stderr, "pshm reader: unsupported version %u in '%s'\n", sb->version,
                         name);
            region_.close();
            return false;
        }

        // Validate header_len
        if (sb->header_len != PSHM_HEADER_LEN) {
            std::fprintf(stderr, "pshm reader: unexpected header_len %u in '%s'\n", sb->header_len,
                         name);
            region_.close();
            return false;
        }

        // Validate contract: dtype, rank, dims, rate_hz, stable_id_hash
        if (sb->dtype != static_cast<uint8_t>(expected_dtype)) {
            std::fprintf(stderr, "pshm reader: dtype mismatch in '%s' (expected %u, got %u)\n",
                         name, static_cast<uint8_t>(expected_dtype), sb->dtype);
            region_.close();
            return false;
        }
        if (sb->rank != expected_rank) {
            std::fprintf(stderr, "pshm reader: rank mismatch in '%s' (expected %u, got %u)\n", name,
                         expected_rank, sb->rank);
            region_.close();
            return false;
        }
        for (uint8_t i = 0; i < expected_rank; ++i) {
            uint32_t exp = expected_dims ? expected_dims[i] : 0;
            if (sb->dims[i] != exp) {
                std::fprintf(stderr,
                             "pshm reader: dim[%u] mismatch in '%s' (expected %u, got %u)\n", i,
                             name, exp, sb->dims[i]);
                region_.close();
                return false;
            }
        }
        // stable_id_hash: log mismatch as a warning but do not reject.
        // In cross-process SHM the writer and reader are compiled from different
        // PDL programs and will naturally have different stable_ids (direction and
        // actor chain differ).  The other contract fields (dtype, rank, dims,
        // slot geometry) are sufficient to ensure data compatibility.
        if (expected_stable_id_hash != 0 && sb->stable_id_hash != expected_stable_id_hash) {
            std::fprintf(stderr,
                         "pshm reader: note: stable_id_hash mismatch in '%s' "
                         "(reader=%lu, writer=%lu) — normal for cross-process SHM\n",
                         name, (unsigned long)expected_stable_id_hash,
                         (unsigned long)sb->stable_id_hash);
        }

        // Geometry match
        if (sb->slot_count != expected_slot_count) {
            std::fprintf(stderr, "pshm reader: slot_count mismatch in '%s' (expected %u, got %u)\n",
                         name, expected_slot_count, sb->slot_count);
            region_.close();
            return false;
        }
        if (sb->slot_payload_bytes != expected_slot_bytes) {
            std::fprintf(stderr,
                         "pshm reader: slot_payload_bytes mismatch in '%s' (expected %u, got %u)\n",
                         name, expected_slot_bytes, sb->slot_payload_bytes);
            region_.close();
            return false;
        }

        slot_count_ = expected_slot_count;
        slot_payload_bytes_ = expected_slot_bytes;
        known_epoch_ = shm_load_acquire(&const_cast<Superblock *>(sb)->epoch);

        // Initialize want_seq to latest available (skip old data)
        uint64_t ws = shm_load_acquire(&const_cast<Superblock *>(sb)->write_seq);
        if (ws >= slot_count_)
            want_seq_ = ws - slot_count_ + 1;
        else
            want_seq_ = (ws > 0) ? 1 : 0;

        valid_ = true;
        return true;
    }

    /// Consume the next available slot's payload into `out`.
    /// Returns payload bytes copied, or 0 if no data / stale.
    size_t consume(void *out, size_t out_bytes) {
        if (!valid_)
            return 0;

        auto *sb = const_cast<Superblock *>(superblock());
        uint64_t latest = shm_load_acquire(&sb->write_seq);

        // No new data
        if (latest < want_seq_ || want_seq_ == 0)
            return 0;

        // Overflow detection: reader too far behind
        if (latest - want_seq_ >= slot_count_) {
            // Fast-forward to latest available
            want_seq_ = latest - slot_count_ + 1;
        }

        size_t slot_stride = sizeof(SlotHeader) + slot_payload_bytes_;
        uint32_t idx = static_cast<uint32_t>(want_seq_ % slot_count_);

        const auto *slot = reinterpret_cast<const SlotHeader *>(
            static_cast<const uint8_t *>(region_.data()) + sizeof(Superblock) + idx * slot_stride);

        // Validate sequence
        uint64_t seen = shm_load_acquire(&const_cast<SlotHeader *>(slot)->seq);
        if (seen != want_seq_) {
            // Race/overwrite: resync to latest
            want_seq_ = latest;
            return 0;
        }

        // Epoch fence detection
        if (slot->flags & FLAG_EPOCH_FENCE) {
            // Epoch boundary: skip this slot, update epoch, resync
            known_epoch_ = slot->epoch;
            want_seq_++;
            // Resync to latest in new epoch
            uint64_t new_latest = shm_load_acquire(&sb->write_seq);
            if (new_latest > want_seq_ && new_latest - want_seq_ >= slot_count_) {
                want_seq_ = new_latest - slot_count_ + 1;
            }
            return 0;
        }

        // Epoch mismatch: resync
        if (slot->epoch != known_epoch_) {
            known_epoch_ = shm_load_acquire(&sb->epoch);
            want_seq_ = latest;
            return 0;
        }

        // Copy payload
        const uint8_t *payload_ptr = reinterpret_cast<const uint8_t *>(slot) + sizeof(SlotHeader);
        size_t copy_bytes = std::min(static_cast<size_t>(slot->payload_bytes), out_bytes);
        std::memcpy(out, payload_ptr, copy_bytes);

        want_seq_++;
        return copy_bytes;
    }

    void close() {
        region_.close();
        valid_ = false;
    }

    bool is_valid() const { return valid_; }

  private:
    const Superblock *superblock() const {
        return reinterpret_cast<const Superblock *>(region_.data());
    }
};

// ── SHM endpoint string parsing ──────────────────────────────────────────────

struct ShmEndpointArgs {
    std::string name;
    int64_t slots = -1;      // -1 = not specified (use constructor default)
    int64_t slot_bytes = -1; // -1 = not specified (use constructor default)
};

/// Parse an SHM endpoint string in two formats:
///   - Spec string: shm("name", slots=N, slot_bytes=M)
///   - Raw name:    "my_ring" or my_ring
inline ShmEndpointArgs parse_shm_endpoint(const std::string &ep) {
    ShmEndpointArgs args;

    // Try spec string format: shm("name", ...)
    auto paren = ep.find('(');
    if (paren != std::string::npos && ep.substr(0, paren) == "shm") {
        // Extract quoted name
        auto q1 = ep.find('"', paren);
        if (q1 != std::string::npos) {
            auto q2 = ep.find('"', q1 + 1);
            if (q2 != std::string::npos)
                args.name = ep.substr(q1 + 1, q2 - q1 - 1);
        }

        // Extract named args: slots=N, slot_bytes=M
        auto extract_named = [&](const char *key) -> int64_t {
            std::string search = std::string(key) + "=";
            auto pos = ep.find(search);
            if (pos == std::string::npos)
                return -1;
            pos += search.size();
            try {
                return std::stoll(ep.substr(pos));
            } catch (...) {
                return -1;
            }
        };
        args.slots = extract_named("slots");
        args.slot_bytes = extract_named("slot_bytes");
    } else {
        // Raw name format — strip quotes if present
        args.name = ep;
        if (args.name.size() >= 2 && args.name.front() == '"' && args.name.back() == '"') {
            args.name = args.name.substr(1, args.name.size() - 2);
        }
    }

    return args;
}

// ── ShmIoAdapter — high-level adapter for generated code ─────────────────────

class ShmIoAdapter {
    const char *name_;
    pipit::net::DType dtype_;
    double rate_hz_;
    uint32_t slots_;      // compile-time immutable geometry
    uint32_t slot_bytes_; // compile-time immutable geometry
    bool is_out_;
    uint64_t stable_id_hash_;
    uint8_t rank_;
    uint32_t dims_[8];
    uint32_t tokens_per_frame_;
    BindState *state_;

    ShmWriter writer_;
    ShmReader reader_;
    bool initialized_ = false;
    int init_fail_count_ = 0;
    static constexpr int MAX_INIT_RETRIES = 3;
    std::string endpoint_;
    std::mutex io_mtx_;

  public:
    ShmIoAdapter(const char *name, bool is_out, pipit::net::DType dtype, double rate_hz,
                 uint32_t slots, uint32_t slot_bytes, const char * /* shm_name — used by codegen */,
                 uint64_t stable_id_hash, uint8_t rank, const uint32_t *dims,
                 uint32_t tokens_per_frame, BindState *state)
        : name_(name), dtype_(dtype), rate_hz_(rate_hz), slots_(slots), slot_bytes_(slot_bytes),
          is_out_(is_out), stable_id_hash_(stable_id_hash), rank_(rank),
          tokens_per_frame_(tokens_per_frame), state_(state) {
        for (uint8_t i = 0; i < 8; ++i)
            dims_[i] = (dims && i < rank) ? dims[i] : 0;
    }

    /// Send data to the SHM ring.
    void send(const void *data, uint32_t n_tokens) {
        std::lock_guard<std::mutex> lk(io_mtx_);
        if (!initialized_)
            lazy_init();
        if (!writer_.is_valid())
            return;

        uint32_t payload_bytes = static_cast<uint32_t>(n_tokens * pipit::net::dtype_size(dtype_));
        writer_.publish(data, payload_bytes, n_tokens, FLAG_FRAME_START | FLAG_FRAME_END,
                        pipit_iteration_index());
    }

    /// Receive data from the SHM ring.
    /// Zero-fills output if no valid data is available.
    void recv(void *out, uint32_t n_tokens) {
        std::lock_guard<std::mutex> lk(io_mtx_);
        size_t fill_bytes = n_tokens * pipit::net::dtype_size(dtype_);
        std::memset(out, 0, fill_bytes);

        if (!initialized_)
            lazy_init();
        if (!reader_.is_valid())
            return;

        reader_.consume(out, fill_bytes);
    }

    /// Validate geometry and reconnect to a new endpoint.
    /// Returns false on geometry mismatch (keeps current mapping).
    bool try_reconnect(const std::string &new_endpoint) {
        std::lock_guard<std::mutex> lk(io_mtx_);

        if (new_endpoint.empty()) {
            // Disconnect: enter no-op mode
            writer_.close();
            reader_.close();
            endpoint_.clear();
            initialized_ = true; // intentional no-op
            return true;
        }

        auto parsed = parse_shm_endpoint(new_endpoint);

        // Validate geometry: reject if new endpoint specifies different slots/slot_bytes
        if (parsed.slots > 0 && static_cast<uint32_t>(parsed.slots) != slots_) {
            std::fprintf(stderr,
                         "pshm bind '%s': rejecting rebind — slots mismatch "
                         "(compile-time=%u, endpoint=%ld)\n",
                         name_, slots_, (long)parsed.slots);
            return false;
        }
        if (parsed.slot_bytes > 0 && static_cast<uint32_t>(parsed.slot_bytes) != slot_bytes_) {
            std::fprintf(stderr,
                         "pshm bind '%s': rejecting rebind — slot_bytes mismatch "
                         "(compile-time=%u, endpoint=%ld)\n",
                         name_, slot_bytes_, (long)parsed.slot_bytes);
            return false;
        }

        // Emit epoch fence on old writer (if active)
        if (is_out_ && writer_.is_valid()) {
            writer_.emit_epoch_fence(pipit_iteration_index());
        }

        // Close old
        writer_.close();
        reader_.close();

        // Open new with constructor's geometry
        endpoint_ = parsed.name;
        initialized_ = false;
        init_fail_count_ = 0;
        lazy_init();
        return true;
    }

  private:
    void lazy_init() {
        // Already holding io_mtx_
        if (init_fail_count_ >= MAX_INIT_RETRIES)
            return;

        // Read current endpoint from BindState
        std::string ep;
        {
            std::lock_guard<std::mutex> lk(state_->mtx);
            ep = state_->current_endpoint;
        }

        auto parsed = parse_shm_endpoint(ep);
        endpoint_ = parsed.name;

        if (endpoint_.empty()) {
            initialized_ = true; // intentional no-op mode
            return;
        }

        bool ok = false;
        if (is_out_) {
            ok = writer_.init(endpoint_.c_str(), slots_, slot_bytes_, dtype_, rank_, dims_,
                              tokens_per_frame_, rate_hz_, stable_id_hash_);
        } else {
            ok = reader_.attach(endpoint_.c_str(), slots_, slot_bytes_, dtype_, rank_, dims_,
                                rate_hz_, stable_id_hash_);
        }

        if (ok) {
            initialized_ = true;
        } else {
            init_fail_count_++;
            std::fprintf(stderr, "pshm bind '%s': failed to open '%s' (attempt %d/%d)\n", name_,
                         endpoint_.c_str(), init_fail_count_, MAX_INIT_RETRIES);
            if (init_fail_count_ >= MAX_INIT_RETRIES) {
                std::fprintf(stderr, "pshm bind '%s': giving up after %d attempts\n", name_,
                             MAX_INIT_RETRIES);
                initialized_ = true; // permanent no-op
            }
        }
    }
};

#else // !defined(__unix__)

// ── Non-POSIX stub ───────────────────────────────────────────────────────────

class ShmIoAdapter {
    const char *name_;
    bool warned_ = false;

  public:
    ShmIoAdapter(const char *name, bool, pipit::net::DType, double, uint32_t, uint32_t,
                 const char *, uint64_t, uint8_t, const uint32_t *, uint32_t, BindState *)
        : name_(name) {}

    void send(const void *, uint32_t) {
        if (!warned_) {
            std::fprintf(stderr,
                         "pshm bind '%s': shared memory transport not supported on this platform\n",
                         name_);
            warned_ = true;
        }
    }

    void recv(void *out, uint32_t n_tokens) {
        if (!warned_) {
            std::fprintf(stderr,
                         "pshm bind '%s': shared memory transport not supported on this platform\n",
                         name_);
            warned_ = true;
        }
        // Zero-fill to maintain non-blocking contract
        std::memset(out, 0, n_tokens * 4); // approximate
    }

    bool try_reconnect(const std::string &) {
        if (!warned_) {
            std::fprintf(stderr,
                         "pshm bind '%s': shared memory transport not supported on this platform\n",
                         name_);
            warned_ = true;
        }
        return false;
    }
};

#endif // defined(__unix__)

} // namespace shm
} // namespace pipit
