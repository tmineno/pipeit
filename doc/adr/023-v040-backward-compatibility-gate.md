# ADR-023: v0.4.0 Backward-Compatibility Gate

## Context

v0.4.0 introduces a large compiler architecture transition (IR boundaries, pass-manager contracts, and ownership refactors). Such changes can unintentionally alter language or CLI behavior.

The project needs a clear default policy for compatibility during architecture migration.

## Decision

Adopt a compatibility gate for v0.4.0:

- Default policy: preserve v0.3.x language and CLI behavior.
- Any breaking change is allowed only if all are provided:
  - explicit spec delta,
  - dedicated ADR documenting rationale and migration,
  - release-note entry with user impact and transition path.

Scope covered by the gate:

- parsing and semantic behavior,
- compiler CLI interface and defaults,
- output-stage behavior (`--emit` variants),
- runtime option behavior in generated binaries.

## Consequences

- Architecture work can proceed while limiting user-facing regressions.
- Breaking behavior becomes intentional and auditable.
- Additional documentation overhead is required for deliberate breaks.

## Alternatives

- Allow incidental behavior changes during refactor: rejected due to regression risk.
- Freeze all behavior including internal contracts: rejected because architecture progress would stall.
- Defer compatibility policy until release phase: rejected due to late discovery risk.

## Exit criteria

- [ ] Compatibility gate is documented in `pcc-spec-v0.4.0.md`.
- [ ] Refactor PRs label behavior-changing deltas explicitly.
- [ ] Any breaking change includes spec+ADR+release-note evidence before merge.
