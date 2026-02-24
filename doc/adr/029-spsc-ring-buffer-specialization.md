# ADR-029: SPSC Ring Buffer Specialization

## Context

`RingBuffer<T, Capacity, Readers>` scans all N reader tails on every write to find the minimum tail position, even when `Readers == 1`. In the current test suite, 100% of inter-task buffers are single-reader (SPSC). The generic multi-reader scan loop adds unnecessary overhead for this common case.

Codegen emits `read(reader_idx, dst, count)` with an explicit `reader_idx` argument for all buffer reads, so any SPSC specialization must preserve this call signature.

## Decision

Add a C++ partial template specialization `RingBuffer<T, Capacity, 1>` to `pipit.h`:

- Single `PaddedTail tail_` instead of `PaddedTail tails_[Readers]` array.
- Writer-private `cached_tail_` eliminates the multi-reader scan loop on fast path.
- Same memory ordering model: release on head/tail stores, acquire on head load (reader) and tail load (writer slow path).

**API compatibility (mandatory)**: The SPSC specialization provides the same API surface as the generic class:

- `bool write(const T* src, std::size_t count)` — same signature.
- `bool read(std::size_t reader_idx, T* dst, std::size_t count)` — keeps `reader_idx` parameter (assert `reader_idx == 0`). Required because codegen emits `_ringbuf.read(reader_idx, dst, count)`.
- `bool read(T* dst, std::size_t count)` — convenience overload.
- `std::size_t available(std::size_t reader_idx = 0) const` — keeps parameter for compatibility.

C++ template instantiation auto-selects the specialization when `Readers == 1`; no codegen changes are required.

Additionally, `LirBufferIo` gains a `reader_count: usize` field populated from `LirInterTaskBuffer::reader_count`. When `reader_count == 1`, codegen emits a `// SPSC: single-reader fast path` comment before the retry loop, documenting intent and providing a hook for future SPSC-specific retry tuning.

## Consequences

- 8–15% buffer throughput improvement for SPSC paths (no tail-scan loop, cached tail on fast path).
- Zero codegen changes required — C++ template instantiation auto-selects.
- New `test_ringbuf.cpp` validates SPSC correctness (single-threaded + multi-threaded stress test).
- LIR snapshot updates for `reader_count` field in `LirBufferIo`.

## Alternatives

- Codegen-time SPSC detection with different emit paths. Rejected: adds codegen complexity; C++ templates solve this at zero maintenance cost.
- Runtime branch on reader count inside generic class. Rejected: branch on every write, worse than template specialization.
- Separate `SpscRingBuffer` class. Rejected: requires codegen to choose between two types; partial specialization is transparent.

## Exit criteria

- `RingBuffer<T, Capacity, 1>` partial specialization compiles and passes correctness tests.
- API-compatible: existing codegen call-sites compile unchanged.
- `test_ringbuf.cpp` SPSC stress tests pass (concurrent writer + single reader).
- `reader_count` field present on `LirBufferIo`, populated during LIR build.
- All existing tests pass unchanged.
