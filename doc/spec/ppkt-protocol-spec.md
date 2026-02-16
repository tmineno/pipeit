# Pipit Packet Protocol (PPKT) Specification

**Version**: 1
**Status**: Draft
**Date**: 2026-02-16

## Goal

Define a simple, self-describing binary datagram protocol for streaming signal data between Pipit pipeline actors and external processes (oscilloscopes, loggers, test harnesses).

## Non-goals

- Reliable delivery (use TCP or application-level ACK if needed)
- Flow control (senders drop on failure; receivers tolerate gaps)
- Encryption or authentication

## Transport

PPKT is transport-agnostic but designed for datagram semantics:

- **UDP** (`AF_INET`, `SOCK_DGRAM`): primary transport for local and remote use
- **Unix domain socket** (`AF_UNIX`, `SOCK_DGRAM`): alternative for local IPC
- All sockets operate in **non-blocking** mode (`O_NONBLOCK`)

Address format in PDL:

- `"host:port"` — resolved as `AF_INET` + `SOCK_DGRAM` (UDP)
- `"unix:///path/to/socket"` — resolved as `AF_UNIX` + `SOCK_DGRAM` (IPC)

## Packet Layout

Each PPKT packet consists of a **48-byte fixed header** followed by a **variable-length payload**.

All multi-byte fields are **little-endian**.

```text
Offset  Size  Field            Type     Description
──────  ────  ─────            ────     ───────────
 0      4B    magic            u8[4]    "PPKT" (0x50, 0x50, 0x4B, 0x54)
 4      1B    version          u8       Protocol version (1)
 5      1B    header_len       u8       Total header size in bytes (48)
 6      1B    dtype            u8       Sample data type (see DType table)
 7      1B    flags            u8       Bitfield (see Flags table)
 8      2B    chan_id           u16le    Channel identifier
10      2B    reserved         u16le    Must be 0x0000
12      4B    sequence         u32le    Per-channel packet sequence number
16      4B    sample_count     u32le    Number of samples in payload
20      4B    payload_bytes    u32le    Payload size in bytes
24      8B    sample_rate_hz   f64le    Task sample rate (Hz)
32      8B    timestamp_ns     u64le    Monotonic timestamp (nanoseconds)
40      8B    iteration_index  u64le    Logical iteration counter
──────  ────  ─────            ────     ───────────
48      var   payload          u8[]     sample_count × dtype_size bytes
```

### DType Table

| Value | Name    | Size   | Layout                              |
|-------|---------|--------|-------------------------------------|
| 0     | f32     | 4B     | IEEE 754 binary32, little-endian    |
| 1     | i32     | 4B     | Two's complement 32-bit, LE        |
| 2     | cf32    | 8B     | `[f32 real, f32 imag]` (C++ `std::complex<float>` layout) |
| 3     | f64     | 8B     | IEEE 754 binary64, little-endian    |
| 4     | i16     | 2B     | Two's complement 16-bit, LE        |
| 5     | i8      | 1B     | Two's complement 8-bit             |
| 6-255 |         |        | Reserved                            |

### Flags Bitfield

| Bit | Name         | Description                                    |
|-----|--------------|------------------------------------------------|
| 0   | first_frame  | First packet in a stream (initial connection)  |
| 1   | last_frame   | Last packet in a stream (graceful shutdown)    |
| 2-7 | reserved     | Must be 0                                      |

## Field Semantics

### `magic`

Fixed value `"PPKT"` (bytes `0x50 0x50 0x4B 0x54`). Receivers MUST validate magic and discard packets that do not match.

### `version`

Protocol version. This specification defines version `1`. Receivers SHOULD accept packets with `version == 1` and MAY reject unknown versions.

### `header_len`

Total header size in bytes, including all fields from offset 0 through the end of `iteration_index`. For version 1, this is always `48`. Future versions MAY increase this value. Receivers MUST use `header_len` to locate the payload start, enabling forward compatibility.

### `dtype`

Identifies the sample data type in the payload. The `payload_bytes` field is redundant with `sample_count × dtype_size` but is included explicitly so that receivers can process payloads without a dtype-to-size lookup table (enables forward compatibility with unknown dtypes).

### `chan_id`

Identifies the logical channel. Multiple actors can share a single UDP port by using different `chan_id` values. The `sequence` number is scoped to each `chan_id`.

### `sequence`

Per-channel monotonically increasing packet sequence number. Starts at `0` for each channel. Wraps from `2^32 - 1` to `0`. Receivers detect wrap by observing `seq < prev_seq`. Receivers detect packet loss by observing `seq > prev_seq + 1`.

### `sample_count` and `payload_bytes`

`sample_count` is the number of samples. `payload_bytes` is the payload size in bytes. For known dtypes: `payload_bytes == sample_count × dtype_size`. Both fields are present for forward compatibility — a receiver that does not recognize a dtype can still skip the payload using `payload_bytes`.

### `sample_rate_hz`

The task's target sample rate in Hz, obtained from `pipit_task_rate_hz()`. This allows receivers to construct a time axis without out-of-band configuration.

### `timestamp_ns`

Monotonic wall-clock timestamp in nanoseconds, obtained from `pipit_now_ns()`. Clock source is `std::chrono::steady_clock` (equivalent to `CLOCK_MONOTONIC` on POSIX). This clock is monotonic but NOT wall-clock aligned — it measures elapsed time since an unspecified epoch (typically system boot).

### `iteration_index`

Logical iteration counter obtained from `pipit_iteration_index()`. Combined with `sample_rate_hz`, this gives the logical time of the first sample in the payload: `t = iteration_index / sample_rate_hz`.

## Sender-Side Chunking

When the payload size exceeds the effective MTU, the sender MUST split the data into multiple self-contained PPKT packets.

**Default MTU**: 1472 bytes (Ethernet 1500 - IP header 20 - UDP header 8).

**Max samples per packet**: `(MTU - 48) / dtype_size`

| DType | dtype_size | Max samples (MTU=1472) |
|-------|-----------|------------------------|
| f32   | 4B        | 356                    |
| i32   | 4B        | 356                    |
| cf32  | 8B        | 178                    |
| f64   | 8B        | 178                    |
| i16   | 2B        | 712                    |
| i8    | 1B        | 1424                   |

Each chunk is a self-contained packet:

- `sample_count` and `payload_bytes` reflect the chunk, not the original frame
- `iteration_index` is adjusted per chunk: `base_iter + offset` where `offset` is the sample offset within the original frame
- `sequence` increments for each packet sent
- `timestamp_ns` is the same for all chunks of one firing (they share the same wall-clock instant)

Receivers do NOT need reassembly logic. Each packet is independently processable.

## Error Handling

### Sender (`socket_write`)

- Socket creation or bind failure: return `ACTOR_ERROR`
- `sendto()` returns `EAGAIN`/`EWOULDBLOCK` or other transient error: silently drop, return `ACTOR_OK`
- The sender MUST NOT block the actor thread

### Receiver (`socket_read`)

- Socket creation or bind failure: return `ACTOR_ERROR`
- `recvfrom()` returns `EAGAIN`/`EWOULDBLOCK` (no data): output zeros, return `ACTOR_OK`
- Invalid magic or unsupported version: discard packet, output zeros, return `ACTOR_OK`
- `payload_bytes` exceeds buffer: discard packet, output zeros, return `ACTOR_OK`
- The receiver MUST NOT block the actor thread

## Examples

### PDL Usage

```pdl
clock 48kHz audio {
    sine(1000, 1.0) | socket_write("localhost:9100", 0)
}

clock 1kHz control {
    socket_read("localhost:9200") | stdout()
}
```

### Packet Example

A single float sample (1.0) sent at 48kHz, iteration 42:

```text
Header (48 bytes):
  50 50 4B 54   magic = "PPKT"
  01            version = 1
  30            header_len = 48
  00            dtype = 0 (f32)
  00            flags = 0
  00 00         chan_id = 0
  00 00         reserved = 0
  2A 00 00 00   sequence = 42
  01 00 00 00   sample_count = 1
  04 00 00 00   payload_bytes = 4
  00 00 00 00 80 70 E7 40   sample_rate_hz = 48000.0
  XX XX XX XX XX XX XX XX   timestamp_ns (varies)
  2A 00 00 00 00 00 00 00   iteration_index = 42

Payload (4 bytes):
  00 00 80 3F   1.0f (IEEE 754 LE)
```

## Acceptance Tests

- [ ] PpktHeader struct is exactly 48 bytes with correct field offsets
- [ ] Sender builds valid PPKT packets for f32, i32, cf32 dtypes
- [ ] Sender chunks payloads exceeding MTU into multiple self-contained packets
- [ ] Receiver validates magic and rejects invalid packets
- [ ] Receiver outputs zeros when no data is available (non-blocking)
- [ ] Loopback test: `socket_write` → `socket_read` on localhost preserves sample values
- [ ] Sequence numbers increment per-channel and wrap correctly
