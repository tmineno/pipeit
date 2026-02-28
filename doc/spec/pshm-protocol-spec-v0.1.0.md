# Pipit Shared Memory Bind Protocol (PSHM) Specification

**Version**: 1
**Status**: Draft
**Date**: 2026-02-23

## Goal

Define a low-latency, non-blocking shared-memory protocol for bind-based data streaming between independent Pipit processes on the same host.

## Non-goals

- Reliable replay/retransmission.
- Inter-host transport.
- Encryption/authentication.

## Transport and Endpoint

PSHM is selected by `bind` endpoint:

```pdl
bind iq = shm("rx.iq", slots=1024, slot_bytes=4096)
```

- `name`: shared memory object name (local host scope).
- `slots`: ring slot count (positive integer).
- `slot_bytes`: payload bytes per slot (positive integer).

`dtype`, `shape`, `rate_hz` are not endpoint options; they are inferred by the compiler and validated against superblock metadata at attach time.

## Topology and Concurrency Model

- One bind endpoint maps to one shared-memory ring.
- Single writer, multiple readers.
- Writer never blocks.
- Reader never blocks.
- Overrun is handled by overwrite/drop semantics, not backpressure.

## Memory Layout

The shared memory object consists of:

1. `Superblock` (fixed 128 bytes)
1. `Slot` array (`slot_count` entries)

Each slot layout:

1. `SlotHeader` (fixed 64 bytes)
1. `payload` (`slot_payload_bytes` bytes)

All multi-byte fields are little-endian.

### Superblock (128 bytes)

| Offset | Size | Field | Type | Description |
|---:|---:|---|---|---|
| 0 | 4 | magic | u8[4] | `"PSHM"` |
| 4 | 1 | version | u8 | protocol version (=1) |
| 5 | 1 | header_len | u8 | superblock size (=128) |
| 6 | 2 | flags | u16le | reserved (0 for v1) |
| 8 | 1 | dtype | u8 | sample type code |
| 9 | 1 | rank | u8 | shape rank (0..8) |
| 10 | 2 | reserved0 | u16le | must be 0 |
| 12 | 4 | tokens_per_frame | u32le | logical tokens per firing |
| 16 | 4 | slot_count | u32le | ring slot count |
| 20 | 4 | slot_payload_bytes | u32le | bytes per slot payload |
| 24 | 8 | rate_hz | f64le | contract rate (tokens/sec domain) |
| 32 | 8 | stable_id_hash | u64le | hash of compiler-generated stable_id |
| 40 | 4 | epoch | u32le (atomic) | rebind generation |
| 44 | 4 | reserved1 | u32le | must be 0 |
| 48 | 8 | write_seq | u64le (atomic) | latest committed sequence |
| 56 | 8 | writer_heartbeat_ns | u64le | monotonic heartbeat |
| 64 | 32 | dims | u32le[8] | shape dims; unused entries = 0 |
| 96 | 8 | endpoint_name_hash | u64le | hash of `shm(name)` |
| 104 | 24 | reserved2 | u8[24] | must be 0 |

### SlotHeader (64 bytes)

| Offset | Size | Field | Type | Description |
|---:|---:|---|---|---|
| 0 | 8 | seq | u64le (atomic) | committed sequence number |
| 8 | 4 | epoch | u32le | generation of this slot |
| 12 | 4 | flags | u32le | bitfield (below) |
| 16 | 8 | iteration_index | u64le | logical iteration index |
| 24 | 8 | timestamp_ns | u64le | monotonic timestamp |
| 32 | 4 | token_count | u32le | tokens in payload |
| 36 | 4 | payload_bytes | u32le | bytes in payload |
| 40 | 24 | reserved | u8[24] | must be 0 |

Flags:

| Bit | Name | Description |
|---:|---|---|
| 0 | frame_start | first chunk of a firing |
| 1 | frame_end | last chunk of a firing |
| 2 | epoch_fence | rebind boundary marker |
| 3-31 | reserved | must be 0 |

## Write Path Semantics

Given `next_seq = write_seq + 1`:

1. Select slot index: `idx = next_seq % slot_count`.
1. Write payload bytes.
1. Write slot metadata fields (`epoch`, `iteration_index`, `timestamp_ns`, `token_count`, `payload_bytes`, `flags`).
1. `store_release(slot.seq, next_seq)`.
1. `store_release(superblock.write_seq, next_seq)`.

Writer MUST NOT block on slow readers. If readers lag more than `slot_count`, old samples are overwritten.

## Read Path Semantics

Reader keeps local `want_seq` (initially `max(1, write_seq - slot_count + 1)`).

1. `latest = load_acquire(superblock.write_seq)`.
1. If `latest < want_seq`: no new data (non-blocking empty read).
1. If `latest - want_seq >= slot_count`: overflow; reader records drop and fast-forwards `want_seq`.
1. Load slot `idx = want_seq % slot_count`.
1. `seen = load_acquire(slot.seq)`.
1. If `seen != want_seq`: treat as race/overwrite, record drop, resync.
1. If valid, consume slot and increment `want_seq`.

Readers MAY skip old data and jump to latest for low-latency display use-cases.

## Attach/Detach Handshake

### Attach

1. Open/map shared memory object.
1. Validate `magic`, `version`, `header_len`.
1. Validate contract (`dtype`, `rank/dims`, `rate_hz`, `stable_id_hash`).
1. On mismatch: attach fails.

### Detach

- Reader/writer unmap and close handle.
- Detach does not require global lock or writer stop.

## Rebind Semantics (Epoch Switch)

Rebind is controlled by language/runtime semantics (iteration-boundary atomic switch). PSHM encodes boundary via `epoch`.

Writer-side requirements:

1. On rebind commit boundary, writer emits one slot with `epoch_fence=1`, `token_count=0`, `payload_bytes=0`.
1. Writer increments `superblock.epoch` with `store_release`.
1. Subsequent data slots use the new epoch value.

Reader-side requirements:

1. Reader detects epoch change via slot `epoch` or superblock `epoch`.
1. Reader drops incomplete pending frame/chunk assembly.
1. Reader resumes from latest committed sequence in new epoch.

## Error Handling

### Writer

- map/create failure: fatal for bind endpoint initialization.
- publish failure due to invalid mapped state: endpoint enters degraded state; pipeline semantics remain non-blocking.

### Reader

- map/open failure: endpoint unavailable (runtime policy decides zero-output vs fatal at startup).
- invalid metadata/mismatch: reject attach or discard slot.
- empty ring state: non-blocking no-data return.

## Memory Ordering Requirements

- Writer publish order MUST use release store on `slot.seq` and `superblock.write_seq`.
- Reader consume order MUST use acquire load on `superblock.write_seq` and `slot.seq`.
- Payload/metadata must be written before `slot.seq` publish.

## Compatibility and Versioning

- This document defines protocol version `1`.
- Receivers SHOULD accept `version == 1`.
- Unknown versions MAY be rejected.
- `header_len` allows forward-compatible superblock extension.

## Security and Safety Considerations

- Implementations SHOULD create shared memory objects with owner-only permissions by default.
- Endpoint names SHOULD be namespaced to avoid collisions.
- Readers MUST validate all size fields before payload access.

## Acceptance Tests

- [ ] Superblock is exactly 128 bytes and SlotHeader is exactly 64 bytes.
- [ ] Writer publishes monotonic `write_seq` and release/acquire contract is respected.
- [ ] Reader detects overwrite when lag exceeds `slot_count`.
- [ ] Contract mismatch (`dtype`, `shape`, `rate_hz`, `stable_id_hash`) is rejected at attach.
- [ ] Rebind emits epoch fence and increments epoch.
- [ ] Reader drops pending frame on epoch change and resumes correctly.
- [ ] Multi-reader scenario (>=2 readers) reads same writer stream without blocking writer.
