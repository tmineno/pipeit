# ADR-024: THIR-First IR Unification Strategy

## Context

v0.4.0 Phase 2 introduces three IR layers (HIR, THIR, LIR) between the parser AST and generated C++. The goal is to eliminate direct AST access from downstream phases (graph, analyze, schedule, codegen) so each phase consumes only its declared inputs.

Currently, `graph.rs` accesses raw AST for define inlining, `analyze.rs` scans `program.statements` for task frequency / param types / modal switch sources, and `codegen.rs` accesses nearly everything. Phase 1 (complete) added stable IDs and snapshot tests as prerequisites.

## Decision

1. **HIR-first define expansion**: Build an HIR normalization pass that expands all `define` calls before graph construction. This removes ~200 lines of define inlining + argument substitution from `graph.rs` and centralizes the expansion logic.

2. **ThirContext wrapper**: Instead of creating a monolithic THIR struct that replaces `ResolvedProgram` / `TypedProgram` / `LoweredProgram`, introduce a `ThirContext` wrapper that borrows existing phase outputs and adds precomputed metadata tables (task info, param types, const values, set directives). This minimizes data reshaping while providing a unified query API.

3. **Sub-phase execution**: Split Phase 2 into sub-phases:
   - **2a**: HIR normalization + ThirContext + migrate graph/analyze/schedule (this ADR)
   - **2b**: LIR struct + migrate codegen to syntax-directed emission
   - **2c**: Migrate type_infer from AST to HIR consumption

4. **CallId allocation for expanded defines**: When the HIR pass expands a define, each actor call in the expanded body receives a fresh `CallId` from the `IdAllocator`. These supplementary IDs are stored in `HirProgram.expanded_call_ids` / `expanded_call_spans` alongside the resolve-phase allocations. Type inference (still consuming AST in Phase 2a) uses resolve-phase IDs; graph/analyze/schedule use HIR-allocated IDs.

5. **Pipeline ordering**: `Parse → Resolve → HIR → TypeInfer → Lower → Graph → ThirContext → Analyze → Schedule → Codegen`. ThirContext construction follows graph construction because param C++ type resolution requires scanning graph nodes for actor param type declarations.

## Consequences

- `graph.rs`, `analyze.rs`, `schedule.rs`, `dim_resolve.rs` no longer accept `&Program` — they consume `&HirProgram` or `&ThirContext`.
- Define expansion is centralized and testable in isolation.
- Precomputed metadata eliminates repeated AST scanning (task freq lookup, param type inference, const value resolution).
- Two CallId namespaces coexist temporarily: resolve-phase IDs (for type_infer) and HIR-expansion IDs (for graph/analyze/schedule). Unified in Phase 2c when type_infer migrates to HIR.
- `codegen.rs` still consumes `&Program` until Phase 2b (LIR migration).

## Alternatives

- **Monolithic THIR struct**: Replace all phase outputs with a single owned struct. Rejected: too much data copying and disruption to existing APIs.
- **Deferred HIR (THIR-first)**: Keep define inlining in graph.rs, only add ThirContext. Rejected: user preference for clean HIR normalization and eliminating graph.rs complexity.
- **All-at-once Phase 2**: Implement HIR + THIR + LIR in a single pass. Rejected: excessive blast radius; sub-phases allow incremental verification.

## Exit Criteria

- [ ] `graph.rs`, `analyze.rs`, `schedule.rs` public APIs do not accept `&Program`
- [ ] All 7 snapshot tests produce byte-identical C++ output
- [ ] HIR roundtrip test: HIR-based graph matches old AST-based graph for all example programs
- [ ] No `program.statements` access in graph/analyze/schedule modules
