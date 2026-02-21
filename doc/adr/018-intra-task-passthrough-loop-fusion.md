# ADR-018: Consolidated Same-Rep Loop Fusion (Actor Baseline + Fork/Probe Passthrough)

## Context

This ADR consolidates two related decisions:

- baseline same-rep chain fusion (actor-only, conservative)
- passthrough extension for `Fork/Probe` inside the same-rep region

The combined goal is to reduce intra-task loop overhead while preserving SDF semantics, FIFO order, observable side-effect order, and existing error behavior.

In unfused output, codegen often emitted multiple consecutive `_r` loops with the same trip count around the same data region.  
This was especially visible in tap-expanded graphs (`fft -> fork -> c2r` and sibling branches like `mag`), where fusion stopped at `Fork/Probe`.

## Decision

### 1. Baseline same-rep fusion (actor-only core)

Apply conservative fusion for contiguous same-rep actor chains:

- equal `repetition_count` and `> 1`
- direct schedule-order adjacency
- direct edge connectivity
- conservative degree checks
- no `Arg::TapRef`
- no back-edge participation

Emit one shared `for (int _r = 0; _r < rep; ++_r)` loop for the chain, preserving call order and existing actor error paths.

### 2. Passthrough extension for `Fork/Probe`

Extend fusion planning so `Fork` and `Probe` can be included as transparent nodes in same-rep regions:

- allowed chain kinds: `Actor`, `Fork`, `Probe`
- appended nodes must be connected from already-fused nodes (connectivity guard)
- keep branch execution single-threaded (no task-internal branch threading in this phase)

### 3. Probe correctness in fused loops

When `Probe` appears inside a fused `_r` loop, emit observation for the per-firing slice (stride-based), not whole-buffer replay per `_r`.

### 4. Semantics and determinism guarantees

- preserve FIFO token order
- preserve observable side-effect order
- preserve `ACTOR_ERROR` short-circuit behavior
- preserve deterministic schedule-order emission

## Consequences

- Fewer loop headers/branches on same-rep regions.
- Better locality in common tap-expanded patterns (`fft/fork/c2r/mag`).
- No new thread-safety/runtime-context risks because execution remains single-threaded inside a task.
- Slightly more complex fusion planner due to passthrough-aware eligibility/connectivity checks.

## Alternatives considered

- Keep actor-only fusion forever: rejected; leaves known tap-expanded optimization gaps.
- Immediate task-internal branch parallelization: rejected for this phase due to safety/effect classification gaps and runtime-context propagation needs.
- Fuse through `Fork` but not `Probe`: rejected; `Probe` is also passthrough in dataflow and can be emitted safely with per-firing slice observation.

## Exit criteria

- [x] Actor-only same-rep fusion baseline implemented.
- [x] `Fork/Probe` passthrough fusion implemented.
- [x] Probe-in-fused-loop per-firing slice emission implemented.
- [x] Codegen tests cover fusion/non-fusion regressions.
- [x] Task-internal branch parallel execution remains deferred.

## Relationship

- `ADR-019` documents the baseline actor-only foundation.
- This ADR is the merged, implementation-facing consolidation used for ongoing work.
