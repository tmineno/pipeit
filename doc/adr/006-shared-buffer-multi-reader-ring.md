# ADR-006: Shared buffer implementation aligned to multi-reader FIFO semantics

## Context

Pipit lang spec §5.7 defines shared memory buffers as bounded asynchronous FIFO channels with:

- Single writer per shared buffer (`-> name`)
- Multiple readers (`@name`) with independent read pointers
- Compile-time bounded sizing (safe upper bound)

The previous implementation had three gaps against that model:

- Runtime ring buffer effectively behaved as single-reader (one read cursor), so multiple readers could interfere
- Generated C++ for repeated firings did not always advance source/destination pointers per firing when doing shared-buffer I/O
- Generated C++ did not check `RingBuffer::read` / `RingBuffer::write` return values, so runtime underflow/overflow could be silent

## Decision

Adopt a single-writer, multi-reader ring buffer model in runtime and emit matching codegen behavior.

- Runtime (`libpipit`):
- `RingBuffer<T, Capacity, Readers>` with `Readers >= 1`
- Maintain one write cursor and one read cursor per reader
- Writer capacity check uses the minimum tail among readers
- Reader API accepts `reader_idx`; keep `read(dst, n)` as reader-0 convenience
- Codegen (`pcc`):
- Emit shared buffers as `RingBuffer<type, capacity, reader_count>`
- Derive `reader_count` from resolved readers of each shared buffer
- Assign deterministic reader indices from sorted reader task names
- Emit `read(reader_idx, ...)` for each task
- In repetition loops, emit pointer offsets using per-firing token stride for both read and write paths
- Check read/write status; on failure, print runtime error, set `_stop = true`, and return from task function
- Validation:
- Add integration tests in `compiler/tests/codegen_compile.rs` for:
- multi-reader index emission
- read/write status checks and stop behavior
- per-firing pointer offset emission in shared-buffer I/O

## Consequences

- Generated C++ now matches spec §5.7 multi-reader semantics (independent read progress)
- Shared buffer boundedness remains explicit and static, consistent with analysis output
- Runtime behavior is fail-fast on shared-buffer I/O contract violations instead of silently continuing
- Ring buffer metadata cost increases with reader count (one tail cursor per reader)
- Deterministic reader index assignment makes generated code stable across runs

## Alternatives

- **Single-reader ring buffer + duplicated writer buffers per reader**: simpler runtime, but higher memory cost and extra copy/fan-out logic in codegen
- **Single-reader semantics with implicit arbitration**: violates spec intent for independent readers and snapshot behavior
- **Blocking read/write on underflow/overflow**: can hide rate/sizing bugs and complicate deterministic scheduling/debugging

## Exit criteria

Revisit if:

- The spec introduces multi-writer shared buffers
- Shared-buffer policy changes from fail-fast to blocking/backpressure semantics
- Reader lifecycle becomes dynamic (runtime attach/detach), requiring non-static reader indexing
