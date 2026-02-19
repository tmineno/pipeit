#pragma once
/// @file std_sink.h
/// @brief Pipit Standard Sink Actor Library
///
/// Sink actors that send signal data to external processes via the
/// Pipit Packet Protocol (PPKT).  See doc/spec/ppkt-protocol-spec-v0.2.x.md.

#include <cstring>
#include <pipit.h>
#include <pipit_net.h>

/// @defgroup sink_actors Sink Actors
/// @{

/// @brief Send signal samples over UDP/IPC using PPKT protocol
///
/// Streams float samples to an external process (oscilloscope, logger, etc.)
/// via non-blocking UDP or Unix domain sockets.  Automatically chunks large
/// payloads to fit within the network MTU.
///
/// Preconditions: N >= 1, addr must be a valid address string
/// Postconditions: Samples sent as PPKT packets (best-effort)
/// Failure modes: Returns ACTOR_ERROR only on socket creation failure.
///   Send failures (EAGAIN, network error) are silently dropped.
/// Side effects: Opens a UDP/IPC socket on first firing (lazy init)
///
/// @param N       Number of input samples per firing
/// @param addr    Destination address ("host:port" for UDP, "unix:///path" for IPC)
/// @param chan_id PPKT channel identifier (for multiplexing on a single port)
/// @return ACTOR_OK on success or silent drop; ACTOR_ERROR on fatal init failure
///
/// Example usage:
/// @code{.pdl}
/// clock 48kHz audio {
///     sine(1000, 1.0) | socket_write("localhost:9100", 0)
/// }
/// @endcode
ACTOR(socket_write, IN(float, N), OUT(void, 0),
      PARAM(std::span<const char>, addr) PARAM(int, chan_id) PARAM(int, N)) {
    (void)out;

    static pipit::net::DatagramSender sender;
    static pipit::net::PpktHeader hdr{};
    static bool initialized = false;

    if (!initialized) {
        if (!sender.open(addr.data(), addr.size())) {
            return ACTOR_ERROR;
        }
        hdr = pipit::net::ppkt_make_header(pipit::net::DTYPE_F32, static_cast<uint16_t>(chan_id));
        hdr.flags = pipit::net::FLAG_FIRST_FRAME;
        initialized = true;
    } else {
        hdr.flags = 0;
    }

    hdr.sample_rate_hz = pipit_task_rate_hz();
    hdr.timestamp_ns = pipit_now_ns();
    hdr.iteration_index = pipit_iteration_index();

    pipit::net::ppkt_send_chunked(sender, hdr, in, static_cast<uint32_t>(N));
    return ACTOR_OK;
}
}
;

/// @}
