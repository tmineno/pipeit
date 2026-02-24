# ADR-022: Unified Diagnostics Model with Codes and Cause Chain

## Context

Diagnostics are emitted across multiple phases, but structure and fidelity differ by phase. This limits:

- consistency of user experience,
- machine-readable tooling integration,
- traceability for propagated failures (e.g., type/shape constraint conflicts).

v0.4.0 architecture work requires a shared diagnostics contract across pass boundaries.

## Decision

Adopt a unified diagnostics payload used by all phases:

- `code`: stable identifier (e.g., `E####` / `W####`),
- `level`: `error` or `warning`,
- `message`: canonical human summary,
- `primary_span`: principal source location,
- `related_spans`: secondary context locations,
- `hint`: optional remediation guidance,
- `cause_chain`: optional linked cause records for propagated failures.

Presentation contract:

- default CLI output stays human-readable,
- structured JSON mode is provided for tooling,
- code meanings are versioned; changing semantics of an existing code requires explicit migration note.

## Consequences

- Better cross-phase consistency and easier debugging.
- Improves IDE/tool integration potential.
- Enables provenance-aware diagnostics in typed/lowered pipeline.
- Requires migration effort in existing diagnostic emitters and tests.

## Alternatives

- Keep phase-local diagnostic formats: rejected due to inconsistency and weak traceability.
- Add JSON output without shared schema: rejected due to unstable tooling contract.
- Only improve formatting without cause chain: rejected because root-cause tracing remains weak.

## Exit criteria

- [x] Shared diagnostic data type is used across core passes.
- [x] Stable code namespace is documented and enforced.
- [x] Primary/related span output is available in human and JSON formats.
- [x] Cause-chain diagnostics exist for propagated constraint failures.
