# ADR-008: Specify optional rate-domain fusion for v0.2.0 scheduling

## Context

In v0.2.0 discussions, generated C++ schedules were questioned when same-rate actor chains were emitted as per-node loops instead of a single fused per-iteration loop.

Both forms are SDF-valid if they preserve iteration semantics and FIFO ordering, but the specification previously did not explicitly separate:

- Language-level semantics (what must be preserved)
- Compiler-level schedule optimizations (what may be transformed)

This caused ambiguity about whether unfused output is a spec violation or an optimization gap.

## Decision

Adopt an explicit two-layer specification for scheduling in v0.2.0:

1. **Language spec rule (Pipit)**  
   Add a normative statement that compilers MAY transform to equivalent static schedules, provided core SDF semantics are preserved.  
   (Added at `pipit-lang-spec-v0.2.x.md` §5.4.4)

2. **Compiler spec rule (pcc)**  
   Define `rate domain` (aka `fusion domain`) as an implementation-level schedule unit and specify when loop fusion is eligible and what invariants MUST hold.  
   (Added at `pcc-spec-v0.2.x.md` §9.1)

3. **Terminology choice**  
   Prefer `rate domain` over `clock domain` for this optimization to avoid confusion with task clock frequency domains.

4. **Spec structure update**  
   Publish compiler spec as `pcc-spec-v0.2.x.md` and apply explicit section numbering for stable cross-references.

## Consequences

- Clarifies that fused and unfused schedules can both be correct under the same language semantics.
- Prevents treating codegen loop shape as a language conformance issue unless semantic invariants are violated.
- Provides concrete compiler-side guidance for implementing and testing fusion eligibility and correctness.
- Improves maintainability of documentation via stable numbered references in `pcc-spec-v0.2.x.md`.

## Alternatives

- **Always require fusion**: rejected; would over-constrain implementation and make legal unfused schedules non-conformant.
- **Leave optimization unspecified**: rejected; keeps ambiguity and recurring interpretation disputes.
- **Use `clock domain` term**: rejected; likely to be confused with multi-clock task semantics already defined in the language.

## Exit criteria

Revisit if:

- Scheduling becomes dynamic/runtime-adaptive rather than static SDF-based
- New observable side effects require stricter ordering constraints than current invariants
- A future spec introduces first-class user controls for fusion groups/domains
