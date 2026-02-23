# ADR-021: Stable Semantic IDs over Span-Keyed Semantic Maps

## Context

Several semantic maps are currently keyed by source spans. Span keys are convenient for diagnostics, but weak as semantic identity when code is transformed (normalization, inlining, cloning, reordering).

v0.4.0 needs stable identity across `AST -> HIR -> THIR -> LIR` so downstream data can be attached and traced without relying on source offsets.

## Decision

Introduce stable semantic IDs and make them primary keys for semantic data:

- assign deterministic IDs for semantic entities (calls, definitions, tasks, nodes, edges),
- propagate origin mapping when entities are rewritten or lowered,
- use spans as location metadata only (not semantic identity keys).

Rules:

- IDs are deterministic for equal input and compiler config.
- Transform passes preserve traceability from derived entities to source-origin IDs.
- APIs between phases exchange IDs for cross-reference, with spans as optional adjunct for diagnostics.

## Consequences

- Improves robustness of cross-pass mapping and provenance tracking.
- Enables reliable diagnostic cause chains and IR snapshot correlation.
- Reduces coupling to source byte offsets and formatting differences.
- Requires refactoring existing span-keyed maps and tests.

## Alternatives

- Keep span keys everywhere: rejected due to identity instability through transforms.
- Use pointer identity: rejected due to non-determinism and serialization constraints.
- Introduce IDs only in late phases: rejected because early-phase traceability remains fragile.

## Exit criteria

- [ ] Span-keyed semantic maps in core pipeline are replaced by stable ID keys.
- [ ] Diagnostic APIs can resolve IDs to spans for user-facing output.
- [ ] ID propagation rules are implemented across normalization/lowering.
- [ ] Deterministic IDs are validated by repeat-compilation tests.
