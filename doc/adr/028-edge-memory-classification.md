# ADR-028: Edge Memory Classification

## Context

LIR edge buffers (`LirEdgeBuffer`) and inter-task buffers (`LirInterTaskBuffer`) have no explicit classification of their memory kind. All intra-task edges are emitted as uniform static arrays regardless of their role (local data flow, passthrough alias, or shared boundary). All inter-task buffers use the generic multi-reader `RingBuffer<T, Capacity, Readers>` template even when only one reader exists.

This lack of classification prevents downstream optimization decisions: alignment hints, SPSC specialization, and future scalarization/restrict annotations all require knowing the edge's memory role at the LIR level.

## Decision

Add a `MemoryKind` enum to both LIR edge types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    Local,   // intra-task, no atomics, local buffer
    Shared,  // inter-task, ring buffer I/O
    Alias,   // passthrough (Fork/Probe), zero-copy
}
```

- `LirEdgeBuffer.memory_kind`: `Local` for normal and feedback edges, `Alias` for passthrough edges.
- `LirInterTaskBuffer.memory_kind`: always `Shared`.

Classification is assigned during LIR build (`build_edge_buffers()`, `build_inter_task_buffers()`) based on existing structural information (alias map, buffer topology).

## Consequences

- LIR Display output includes memory kind annotation for debuggability.
- Foundation for alignment decisions (ADR-028 Commit 2: `alignas(64)` for `Local` non-feedback edges).
- Foundation for SPSC detection (ADR-029: `Shared` buffers with `reader_count == 1`).
- LIR snapshot tests require updates.
- No codegen behavioral change in this commit — classification is metadata only.

## Alternatives

- Infer memory kind at codegen time instead of storing in LIR. Rejected: violates LIR's "self-contained, pre-resolved" contract (ADR-025).
- Use a boolean `is_shared` flag. Rejected: three-way classification is needed for alias vs local distinction.

## Exit criteria

- `MemoryKind` field present on both `LirEdgeBuffer` and `LirInterTaskBuffer`.
- LIR snapshots updated and passing.
- All existing tests pass unchanged.
