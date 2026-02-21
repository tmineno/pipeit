# ADR-025: LIR Backend IR for Syntax-Directed Codegen

## Context

Phase 2a (ADR-024) introduced HIR and ThirContext, migrating graph/analyze/schedule off raw AST. Codegen (`codegen.rs`, 4,101 LOC) remains the last major AST consumer. It takes 8 borrowed references (`program`, `resolved`, `graph`, `analysis`, `schedule`, `registry`, `options`, `lowered`) and performs extensive type/rate/dimension inference inline during C++ emission.

This inline resolution makes codegen fragile, hard to test in isolation, and tightly coupled to every upstream phase. Approximately 600 LOC of codegen performs inference that duplicates or extends analysis/schedule logic.

## Decision

1. **Self-contained LIR**: Introduce a Low-level IR (`LirProgram`) that pre-resolves all types, rates, dimensions, buffer metadata, and actor parameters. Codegen receives only `&LirProgram` + `&CodegenOptions` — no other phase outputs.

2. **ThirContext-based builder**: `build_lir(thir, graph, analysis, schedule, options) -> LirProgram`. No `&Program` needed — all AST data comes through ThirContext (const values via HirConst, param types precomputed, set directives precomputed, buffer topology via resolved).

3. **Structured data, not pre-formatted C++**: LIR stores typed values (`cpp_type`, literals, token counts, rates) and structured argument lists (`Vec<LirActorArg>` with 6 variants: Literal, ParamRef, ConstScalar, ConstSpan, ConstArrayLen, DimValue). Codegen formats C++ syntax from this structured data. No pre-formatted C++ fragments in LIR.

4. **Canonical ordering**: All LIR collections use deterministic ordering (sorted by name or node ID) to guarantee reproducible output. Edge buffers sorted by `(src_node_id, tgt_node_id)`, tasks sorted by name, inter-task buffers sorted by name.

5. **Three-way timer_spin distinction**: `LirTimerSpin` enum with `Fixed(i64)` and `Adaptive` variants. The LIR builder inspects raw `thir.set_directive("timer_spin")` SetValue rather than `thir.timer_spin: Option<f64>`, because the latter cannot distinguish "not set" from `set timer_spin = auto` (both return None).

6. **Incremental migration**: One logical group per commit (globals, task structure, firings, modal/ctrl), with 7 insta snapshot tests as safety net ensuring byte-identical C++ output throughout.

## Consequences

- `codegen.rs` shrinks by ~1,500 LOC (4,101 → ~2,600, 37% reduction). All type/rate/dimension inference moves to the LIR builder.
- Codegen becomes syntax-directed: it reads pre-resolved data and emits C++ without querying upstream phases.
- `CodegenCtx` fields narrow from 8 borrowed references to `lir: &LirProgram` + `options: &CodegenOptions`.
- LIR builder centralizes resolution logic that was scattered across codegen helper functions (`format_actor_params`, `resolve_missing_param_value`, `infer_edge_wire_type`, etc.).
- Pipeline gains one additional step: `... → Schedule → LIR → Codegen`.
- LIR is independently testable — builder correctness can be verified without running codegen.

## Alternatives

- **Direct ThirContext consumption by codegen**: Skip LIR, have codegen read ThirContext + analysis + schedule directly. Rejected: codegen would still need graph/analysis/schedule access and inline resolution; doesn't achieve syntax-directed emission.
- **Pre-formatted C++ strings in LIR**: Store actor params as formatted C++ strings. Rejected: violates separation of concerns, makes LIR untestable without C++ knowledge, fragile to formatting changes.
- **Big-bang migration**: Rewrite codegen in one commit. Rejected: too risky for a 4,101-LOC file; incremental migration with snapshot tests is safer.

## Exit Criteria

- [ ] `codegen.rs` public API accepts only `&LirProgram` + `&CodegenOptions`
- [ ] All 7 snapshot tests produce byte-identical C++ output
- [ ] No `program.statements` access in codegen module
- [ ] No `&Program`, `&ResolvedProgram`, `&ProgramGraph`, `&AnalyzedProgram`, `&ScheduledProgram` in `CodegenCtx`
- [ ] LIR builder has unit tests for const/param/directive/buffer/task resolution
