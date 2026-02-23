# ADR-020: Pass Manager Artifact Model and Invalidation

## Context

The current compiler pipeline is mostly hard-wired in driver order. This makes it hard to:

- evaluate only the passes required for a specific `--emit` target,
- reason about pass ownership boundaries,
- introduce deterministic artifact caching.

For v0.4.0 architecture work, pass dependencies and cache invalidation must be explicit and machine-checkable.

## Decision

Adopt a pass-manager contract where each pass declares:

- `inputs`: required artifacts and config fields,
- `outputs`: produced artifacts,
- `invariants`: pre/post conditions,
- `invalidation_key`: deterministic hash of semantic dependencies.

Pass evaluation is dependency-driven:

- requested output (`--emit ...`) maps to required artifacts,
- manager resolves transitive dependencies,
- only minimal required passes are evaluated.

Caching contract:

- cache entries are keyed by pass `invalidation_key`,
- provenance artifacts (registry source, manifest/header hashes, schema version) participate in keys,
- cache hit must be semantically equivalent to recomputation.

## Consequences

- Pass boundaries become explicit and auditable.
- Future incremental/cached compilation becomes feasible without hidden coupling.
- Adding/changing a pass requires declaring dependency and invalidation impacts.
- Initial implementation overhead increases due to artifact plumbing.

## Alternatives

- Keep fixed sequential orchestration in `main`: rejected due to weak dependency visibility and limited optimization headroom.
- Ad-hoc per-pass caches without shared contract: rejected due to drift risk and invalidation inconsistency.
- Build-system-only caching (outside compiler): rejected because internal semantic artifacts still need ownership/invalidation rules.

## Exit criteria

- [ ] Pass registry exists with explicit `inputs`/`outputs`/`invalidation_key`.
- [ ] `--emit` modes are resolved via dependency graph, not hard-coded full pipeline.
- [ ] Artifact provenance (manifest/header hashes + schema version) is part of invalidation.
- [ ] Cache hit vs recompute produces identical semantic outputs/diagnostics.
