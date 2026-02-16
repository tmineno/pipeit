#pragma once
/// @file pipit_net.h
/// @brief Pipit Packet Protocol (PPKT) header and datagram transport
///
/// Provides the PPKT wire format and non-blocking UDP / Unix domain socket
/// wrappers for streaming signal data between Pipit actors and external
/// processes.  See doc/spec/ppkt-protocol-spec.md for the full specification.

#include <arpa/inet.h>
#include <cstdint>
#include <cstring>
#include <fcntl.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include <pipit.h>

namespace pipit {
namespace net {

// ── PPKT Header (48 bytes, little-endian) ───────────────────────────────────

static constexpr uint8_t PPKT_MAGIC[4] = {'P', 'P', 'K', 'T'};
static constexpr uint8_t PPKT_VERSION = 1;
static constexpr uint8_t PPKT_HEADER_LEN = 48;

enum DType : uint8_t {
    DTYPE_F32 = 0,
    DTYPE_I32 = 1,
    DTYPE_CF32 = 2,
    DTYPE_F64 = 3,
    DTYPE_I16 = 4,
    DTYPE_I8 = 5,
};

inline size_t dtype_size(DType dt) {
    switch (dt) {
    case DTYPE_F32:
        return 4;
    case DTYPE_I32:
        return 4;
    case DTYPE_CF32:
        return 8;
    case DTYPE_F64:
        return 8;
    case DTYPE_I16:
        return 2;
    case DTYPE_I8:
        return 1;
    default:
        return 0;
    }
}

enum Flags : uint8_t {
    FLAG_FIRST_FRAME = 1 << 0,
    FLAG_LAST_FRAME = 1 << 1,
};

#pragma pack(push, 1)
struct PpktHeader {
    uint8_t magic[4];         //  0: "PPKT"
    uint8_t version;          //  4: protocol version (1)
    uint8_t header_len;       //  5: total header size (48)
    uint8_t dtype;            //  6: sample data type
    uint8_t flags;            //  7: bitfield
    uint16_t chan_id;         //  8: channel identifier
    uint16_t reserved;        // 10: must be 0
    uint32_t sequence;        // 12: per-channel seq number
    uint32_t sample_count;    // 16: number of samples
    uint32_t payload_bytes;   // 20: payload size in bytes
    double sample_rate_hz;    // 24: task rate (Hz)
    uint64_t timestamp_ns;    // 32: steady_clock nanoseconds
    uint64_t iteration_index; // 40: logical iteration counter
};
#pragma pack(pop)

static_assert(sizeof(PpktHeader) == 48, "PpktHeader must be exactly 48 bytes");

/// Initialize a PpktHeader with default values.
inline PpktHeader ppkt_make_header(DType dtype, uint16_t chan_id) {
    PpktHeader h{};
    std::memcpy(h.magic, PPKT_MAGIC, 4);
    h.version = PPKT_VERSION;
    h.header_len = PPKT_HEADER_LEN;
    h.dtype = static_cast<uint8_t>(dtype);
    h.flags = 0;
    h.chan_id = chan_id;
    h.reserved = 0;
    h.sequence = 0;
    h.sample_count = 0;
    h.payload_bytes = 0;
    h.sample_rate_hz = 0.0;
    h.timestamp_ns = 0;
    h.iteration_index = 0;
    return h;
}

/// Validate a received PpktHeader.  Returns true if magic and version match.
inline bool ppkt_validate(const PpktHeader &h) {
    return std::memcmp(h.magic, PPKT_MAGIC, 4) == 0 && h.version == PPKT_VERSION;
}

// ── Address parsing ─────────────────────────────────────────────────────────

enum class AddrKind { INET, UNIX, INVALID };

struct ParsedAddr {
    AddrKind kind;
    struct sockaddr_storage storage;
    socklen_t len;
};

/// Parse an address string into a sockaddr.
///
/// Formats:
///   "host:port"            → AF_INET + SOCK_DGRAM (UDP)
///   "unix:///path/to/sock" → AF_UNIX + SOCK_DGRAM (IPC)
inline ParsedAddr parse_address(const char *addr, size_t addr_len) {
    ParsedAddr result{};
    result.kind = AddrKind::INVALID;
    std::memset(&result.storage, 0, sizeof(result.storage));

    // Build null-terminated string
    char buf[256];
    size_t n = (addr_len < sizeof(buf) - 1) ? addr_len : sizeof(buf) - 1;
    std::memcpy(buf, addr, n);
    buf[n] = '\0';

    // Unix domain socket: "unix:///path"
    if (n >= 7 && std::strncmp(buf, "unix://", 7) == 0) {
        const char *path = buf + 7;
        auto *un = reinterpret_cast<struct sockaddr_un *>(&result.storage);
        un->sun_family = AF_UNIX;
        size_t path_len = std::strlen(path);
        if (path_len >= sizeof(un->sun_path))
            return result; // path too long
        std::memcpy(un->sun_path, path, path_len + 1);
        result.len = static_cast<socklen_t>(offsetof(struct sockaddr_un, sun_path) + path_len + 1);
        result.kind = AddrKind::UNIX;
        return result;
    }

    // UDP: "host:port"
    const char *colon = std::strrchr(buf, ':');
    if (!colon || colon == buf)
        return result; // no port

    char host[128];
    size_t host_len = static_cast<size_t>(colon - buf);
    if (host_len >= sizeof(host))
        return result;
    std::memcpy(host, buf, host_len);
    host[host_len] = '\0';

    int port = std::atoi(colon + 1);
    if (port <= 0 || port > 65535)
        return result;

    auto *in = reinterpret_cast<struct sockaddr_in *>(&result.storage);
    in->sin_family = AF_INET;
    in->sin_port = htons(static_cast<uint16_t>(port));

    if (std::strcmp(host, "localhost") == 0) {
        in->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    } else {
        if (inet_pton(AF_INET, host, &in->sin_addr) != 1)
            return result;
    }

    result.len = sizeof(struct sockaddr_in);
    result.kind = AddrKind::INET;
    return result;
}

// ── DatagramSender (non-blocking) ───────────────────────────────────────────

class DatagramSender {
    int fd_ = -1;
    struct sockaddr_storage addr_{};
    socklen_t addr_len_ = 0;
    bool valid_ = false;

  public:
    DatagramSender() = default;

    /// Open a non-blocking datagram socket for sending.
    bool open(const char *addr, size_t addr_len) {
        ParsedAddr pa = parse_address(addr, addr_len);
        if (pa.kind == AddrKind::INVALID)
            return false;

        int domain = (pa.kind == AddrKind::UNIX) ? AF_UNIX : AF_INET;
        fd_ = ::socket(domain, SOCK_DGRAM, 0);
        if (fd_ < 0)
            return false;

        // Set non-blocking
        int flags = fcntl(fd_, F_GETFL, 0);
        if (flags < 0 || fcntl(fd_, F_SETFL, flags | O_NONBLOCK) < 0) {
            ::close(fd_);
            fd_ = -1;
            return false;
        }

        addr_ = pa.storage;
        addr_len_ = pa.len;
        valid_ = true;
        return true;
    }

    /// Send data.  Returns true on success, false on any error (non-blocking:
    /// EAGAIN/EWOULDBLOCK is treated as a silent drop).
    bool send(const void *data, size_t len) {
        if (!valid_)
            return false;
        ssize_t r = ::sendto(fd_, data, len, 0, reinterpret_cast<const struct sockaddr *>(&addr_),
                             addr_len_);
        return r >= 0;
    }

    bool is_valid() const { return valid_; }

    ~DatagramSender() {
        if (fd_ >= 0)
            ::close(fd_);
    }

    // Non-copyable
    DatagramSender(const DatagramSender &) = delete;
    DatagramSender &operator=(const DatagramSender &) = delete;
};

// ── DatagramReceiver (non-blocking) ─────────────────────────────────────────

class DatagramReceiver {
    int fd_ = -1;
    bool valid_ = false;

  public:
    DatagramReceiver() = default;

    /// Open and bind a non-blocking datagram socket for receiving.
    bool open(const char *addr, size_t addr_len) {
        ParsedAddr pa = parse_address(addr, addr_len);
        if (pa.kind == AddrKind::INVALID)
            return false;

        int domain = (pa.kind == AddrKind::UNIX) ? AF_UNIX : AF_INET;
        fd_ = ::socket(domain, SOCK_DGRAM, 0);
        if (fd_ < 0)
            return false;

        // Allow address reuse for UDP
        if (domain == AF_INET) {
            int optval = 1;
            setsockopt(fd_, SOL_SOCKET, SO_REUSEADDR, &optval, sizeof(optval));
        }

        // Bind
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

        valid_ = true;
        return true;
    }

    /// Receive data.  Returns number of bytes received, or -1 on error.
    /// Returns 0 if no data is available (non-blocking).
    ssize_t recv(void *buf, size_t max_len) {
        if (!valid_)
            return -1;
        ssize_t r = ::recvfrom(fd_, buf, max_len, 0, nullptr, nullptr);
        if (r < 0) {
            if (errno == EAGAIN || errno == EWOULDBLOCK)
                return 0; // no data available
            return -1;    // actual error
        }
        return r;
    }

    bool is_valid() const { return valid_; }

    ~DatagramReceiver() {
        if (fd_ >= 0)
            ::close(fd_);
    }

    // Non-copyable
    DatagramReceiver(const DatagramReceiver &) = delete;
    DatagramReceiver &operator=(const DatagramReceiver &) = delete;
};

// ── Chunked send helper ─────────────────────────────────────────────────────

/// Default MTU: Ethernet 1500 - IP header 20 - UDP header 8.
static constexpr size_t PPKT_DEFAULT_MTU = 1472;

/// Send N samples as one or more PPKT packets with automatic chunking.
///
/// Each chunk is a self-contained packet.  If the total payload fits in one
/// MTU-sized packet, a single packet is sent.  Otherwise, the data is split
/// into multiple packets.
///
/// @param sender   Opened DatagramSender
/// @param hdr      Base header (sequence and iteration_index will be updated)
/// @param data     Sample data pointer
/// @param n        Total number of samples
/// @param mtu      Maximum packet size (default: PPKT_DEFAULT_MTU)
/// @return         Number of packets sent (0 on failure)
inline int ppkt_send_chunked(DatagramSender &sender, PpktHeader &hdr, const void *data, uint32_t n,
                             size_t mtu = PPKT_DEFAULT_MTU) {
    size_t dsz = dtype_size(static_cast<DType>(hdr.dtype));
    if (dsz == 0)
        return 0;

    size_t max_payload = mtu - sizeof(PpktHeader);
    uint32_t max_samples = static_cast<uint32_t>(max_payload / dsz);
    if (max_samples == 0)
        return 0;

    // Packet buffer: header + payload
    alignas(8) uint8_t pkt[65536];

    uint64_t base_iter = hdr.iteration_index;
    int packets_sent = 0;
    uint32_t offset = 0;

    while (offset < n) {
        uint32_t chunk = (n - offset < max_samples) ? (n - offset) : max_samples;
        hdr.sample_count = chunk;
        hdr.payload_bytes = static_cast<uint32_t>(chunk * dsz);
        hdr.iteration_index = base_iter + offset;

        size_t pkt_size = sizeof(PpktHeader) + hdr.payload_bytes;
        std::memcpy(pkt, &hdr, sizeof(PpktHeader));
        std::memcpy(pkt + sizeof(PpktHeader), static_cast<const uint8_t *>(data) + offset * dsz,
                    hdr.payload_bytes);

        if (sender.send(pkt, pkt_size))
            packets_sent++;

        hdr.sequence++;
        offset += chunk;
    }

    return packets_sent;
}

} // namespace net
} // namespace pipit
