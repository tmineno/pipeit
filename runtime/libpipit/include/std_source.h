#pragma once
/// @file std_source.h
/// @brief Pipit Standard Source Actor Library
///
/// Source actors that receive signal data from external processes via the
/// Pipit Packet Protocol (PPKT).  See doc/spec/ppkt-protocol-spec-v0.3.0.md.

#include <cstring>
#include <pipit.h>
#include <pipit_net.h>

/// @defgroup source_actors Source Actors
/// @{

/// @brief Receive signal samples over UDP/IPC using PPKT protocol
///
/// Receives float samples from an external process via non-blocking UDP or
/// Unix domain sockets.  When no data is available, outputs zeros to keep
/// the SDF schedule running.
///
/// Preconditions: N >= 1, addr must be a valid address string
/// Postconditions: Output buffer filled with received samples or zeros
/// Failure modes: Returns ACTOR_ERROR only on socket bind failure.
///   Missing data results in zero-filled output with ACTOR_OK.
/// Side effects: Binds a UDP/IPC socket on first firing (lazy init)
///
/// @param N    Number of output samples per firing
/// @param addr Listen address ("host:port" for UDP, "unix:///path" for IPC)
/// @return ACTOR_OK on success or no-data; ACTOR_ERROR on fatal init failure
///
/// Example usage:
/// @code{.pdl}
/// clock 1kHz control {
///     socket_read("localhost:9200") | stdout()
/// }
/// @endcode
ACTOR(socket_read, IN(void, 0), OUT(float, N), PARAM(std::span<const char>, addr) PARAM(int, N)) {
    (void)in;

    static pipit::net::DatagramReceiver receiver;
    static bool initialized = false;

    if (!initialized) {
        if (!receiver.open(addr.data(), addr.size())) {
            return ACTOR_ERROR;
        }
        initialized = true;
    }

    // Max receivable packet: header + N floats
    constexpr size_t MAX_PKT = sizeof(pipit::net::PpktHeader) + 4096 * sizeof(float);
    static uint8_t recv_buf[MAX_PKT > 65536 ? 65536 : MAX_PKT];

    // Zero-fill output as default (no data available)
    std::memset(out, 0, static_cast<size_t>(N) * sizeof(float));

    // Drain all available packets, keeping the latest one
    // (in case sender is faster than receiver)
    ssize_t last_n = 0;
    for (;;) {
        ssize_t n = receiver.recv(recv_buf, sizeof(recv_buf));
        if (n <= 0)
            break;
        last_n = n;
    }

    if (last_n <= 0)
        return ACTOR_OK; // no data — output zeros

    // Validate header
    if (static_cast<size_t>(last_n) < sizeof(pipit::net::PpktHeader))
        return ACTOR_OK; // too short — output zeros

    pipit::net::PpktHeader hdr;
    std::memcpy(&hdr, recv_buf, sizeof(pipit::net::PpktHeader));

    if (!pipit::net::ppkt_validate(hdr))
        return ACTOR_OK; // invalid header — output zeros

    // Copy payload samples
    size_t available_bytes = static_cast<size_t>(last_n) - sizeof(pipit::net::PpktHeader);
    size_t available_samples = available_bytes / sizeof(float);
    size_t copy_count =
        (available_samples < static_cast<size_t>(N)) ? available_samples : static_cast<size_t>(N);

    if (copy_count > 0) {
        std::memcpy(out, recv_buf + sizeof(pipit::net::PpktHeader), copy_count * sizeof(float));
    }

    return ACTOR_OK;
}
}
;

/// @}
