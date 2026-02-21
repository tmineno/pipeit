# ADR-019: Same-Rep Chain Loop Fusion (Actor-Only Baseline)

## Context

Before this work, codegen emitted per-node repetition loops even when adjacent actors had the same `repetition_count`.  
Typical output looked like multiple consecutive `for (int _r ...)` loops with the same trip count, increasing loop-control overhead and reducing locality.

The language/compiler specs allow equivalent static-schedule transformations as long as FIFO order, observable side-effect order, and error behavior are preserved.

## Decision

Introduce a conservative baseline loop-fusion pass in codegen:

- target only adjacent `Actor -> Actor` pairs/chains
- require equal `repetition_count` and `> 1`
- require direct edge adjacency
- require conservative degree checks (`left.outgoing == 1`, `right.incoming == 1`, `right.outgoing == 1`)
- reject `Arg::TapRef` actors
- reject nodes touching feedback back-edges
- preserve existing actor error handling (`ACTOR_OK` checks, `_stop/_exit_code` behavior)

The fused region is emitted as a single `for (int _r = 0; _r < rep; ++_r)` loop with actor calls in schedule order.

## Consequences

- Reduced loop overhead on straight same-rep actor chains.
- Kept semantics safe by using strict eligibility gates.
- Left optimization gaps for tap-expanded graphs (`Fork/Probe` boundaries blocked fusion).

## Alternatives considered

- Always fuse all same-rep nodes: rejected due to semantics/safety risk.
- Immediate branch-level parallelization: rejected; out of scope for baseline and requires stronger safety metadata.
- Keep unfused form: rejected due to avoidable overhead on common chains.

## Follow-up

This baseline ADR is merged and extended by `ADR-018` to cover passthrough (`Fork/Probe`) fusion in single-thread safe-first mode.
