# Feature: pcc — Pipit Compiler Collection

Version: 0.4.0

## 1. Goal

`pcc` compiles Pipit Definition Language (`.pdl`) programs into generated C++ or executables while preserving static SDF guarantees and deterministic behavior.

v0.4.0 achieves:

- Explicit IR boundaries (`AST → HIR → THIR → LIR → C++`) with each phase consuming declared artifacts only
- Dependency-driven pass manager with minimal-pass evaluation per `--emit` target
- Unified diagnostics model with stable codes and machine-readable JSON output
- Stage-scoped verification framework (HIR H1-H3, Lower L1-L5, Schedule S1-S2, LIR R1-R2)
- Runtime shell library (`pipit_shell.h`) replacing inline CLI/stats/probe shell in generated C++
- Registry determinism: `--emit manifest` for hermetic metadata generation, `--emit build-info` for provenance auditing

Refer to [pipit-lang-spec-v0.3.0](pipit-lang-spec-v0.3.0.md) for language semantics. This document specifies compiler tool behavior and architecture contracts.

## 2. Non-goals

- No language-surface expansion (syntax/semantics remain aligned with v0.3.x).
- No mandatory incremental/watch mode.
- No mandatory distributed build cache.
- No C++ AST parsing of actor implementation bodies.
- No CMake build integration (deferred to Phase 7b).

---

## 3. Compatibility Gate (v0.3.x Baseline)

v0.4.0 adopts a compatibility gate:

- Default behavior keeps v0.3.x language and CLI compatibility.
- Any breaking behavior requires all of:
  - explicit spec delta in this file (or successor spec),
  - dedicated ADR with migration reasoning,
  - release-note entry with impact and migration path.

Compatibility gate scope includes:

- language parsing/typing behavior,
- `pcc` CLI options and defaults,
- output-stage semantics (`--emit`),
- runtime option behavior in generated binaries (`--duration`, `--param`, `--probe`, `--probe-output`, `--stats`).

**Known breaking changes in v0.4.0:**

- `--emit cpp` without `-o` now writes to **stdout** (was: `a.out`). See §5.2.1.

---

## 4. Architecture Contract

### 4.1 IR Boundaries

v0.4.0 defines four compiler IR stages:

1. **AST**: Parsed syntax with spans. No semantic resolution.
2. **HIR** (resolved/normalized): Name resolution complete. Structural normalization (define expansion, task/mode normalization, explicit tap/buffer semantics). Built by `hir.rs`.
3. **THIR** (typed/lowered): Type inference and monomorphization complete. Safe implicit widening materialized as explicit nodes. Proof obligations validated (L1-L5). Accessed via `ThirContext` wrapper (`thir.rs`) which provides unified query API over HIR + resolved + typed + lowered + registry + precomputed metadata.
4. **LIR** (scheduled/backend-ready): All types, rates, dimensions, buffer metadata, and actor params pre-resolved. Backend is syntax-directed from LIR (no semantic re-inference). Built by `lir.rs` (~2,050 LOC); codegen reads LIR only (~2,630 LOC).

Canonical phase flow:

```text
source → Parse → AST → Resolve → build_hir → HIR → type_infer → lower → Graph → ThirContext → Analyze → Schedule → build_lir → LIR → Codegen → C++
```

### 4.2 Pass Ownership

| Pass | Input IR | Output IR/Artifact | Owns |
|---|---|---|---|
| Parse | source text | `AST` (Program) | grammar/lexing |
| Resolve | `AST`, registry | ResolvedProgram | symbols, scope |
| BuildHir | `AST`, ResolvedProgram | `HIR` (HirProgram) | define expansion, normalization |
| TypeInfer | `HIR`, registry | TypedResult | typing, monomorphization |
| Lower | `HIR`, TypedResult | LoweredResult + Cert(L1-L5) | widening insertion, proof obligations |
| BuildGraph | `HIR` | GraphResult | SDF graph, edges, back-edges |
| Analyze | ThirContext, GraphResult | AnalysisResult | rates, shapes, buffer layout |
| Schedule | ThirContext, GraphResult, AnalysisResult | ScheduleResult | task ordering, K-factors |
| BuildLir | ThirContext, GraphResult, AnalysisResult, ScheduleResult | `LIR` (LirProgram) | backend IR construction |
| Codegen | `LIR`, GraphResult, ScheduleResult, CodegenOptions | C++ source | serialization only |

Rules:

- A pass may consume only declared artifacts.
- A pass may not re-infer semantics owned by earlier passes.
- Cross-pass data sharing is via artifacts, not hidden side channels.

### 4.3 Pass Manager Contract

Each pass declares (`pass.rs`):

- `id`: unique `PassId` (9 passes: Parse, Resolve, BuildHir, TypeInfer, Lower, BuildGraph, Analyze, Schedule, Codegen),
- `inputs`: required `ArtifactId`s (11 artifacts),
- `outputs`: produced `ArtifactId`s,
- `invariants`: pre/post conditions.

`--emit` targets resolve required artifacts via `required_passes(terminal)` topological walk and evaluate the minimal pass subset. For example, `--emit graph-dot` skips TypeInfer/Lower entirely.

Pipeline orchestration (`pipeline.rs`): `CompilationState` with borrow-split artifacts, `run_pipeline()` with `on_pass_complete` callback for diagnostic reporting.

### 4.4 Artifact/Caching Contract

- Artifact keys are deterministic across machines for equal inputs/config.
- Registry provenance participates in invalidation (via canonical JSON fingerprint).
- Cache miss or verification failure falls back to recompute.
- Cache behavior must not change observable compiler semantics.
- **Note**: Invalidation key hashing and reusable cache are deferred to Phase 3b/3c.

### 4.5 Shell Library

Generated C++ uses a descriptor-table pattern with `pipit_shell.h`:

- Codegen emits compact descriptor arrays: `ProgramDesc`, `TaskDesc[]`, `ParamDesc[]`, `ProbeDesc[]`, `BufferStatsDesc[]`
- `main()` calls `pipit::shell_main(desc)` (~25 LOC vs ~150 LOC inline shell)
- Runtime shell handles: CLI parsing, probe init, duration wait, stats printing, thread launch
- All runtime flags (`--duration`, `--param`, `--probe`, `--probe-output`, `--stats`) handled by shell library
- Task function bodies (actor firings, edge I/O, modal logic) remain in generated code

### 4.6 Verification Framework

Stage-scoped verification via `StageCert` trait (`pass.rs`):

| Stage | Obligations | Evidence |
|---|---|---|
| HIR | H1 (no raw defines), H2 (unique CallIds), H3 (call-node coverage) | `hir::HirCert` |
| Lower | L1 (type consistency), L2 (widening safety), L3 (coverage), L4 (completeness), L5 (no unresolved params) | `lower::Cert` |
| Schedule | S1 (complete coverage), S2 (topological order) | `schedule::ScheduleCert` |
| LIR | R1 (program completeness), R2 (deterministic emission) | `lir::LirCert` |

Verification runs in debug profile (`cargo test`); release matrix deferred to Phase 8.

---

## 5. Inputs and Outputs

### 5.1 Inputs

- `.pdl` source (required for all stages except `manifest`),
- actor metadata:
  - preferred: `--actor-meta <path>` manifest (`actors.meta.json`),
  - fallback: header scanning via `-I` / `--actor-path`,
- compilation config (`--emit`, `--cc`, `--cflags`, `--release`, etc.).

### 5.2 Outputs

| `--emit` stage | Description | Requires `.pdl` |
|---|---|---|
| `exe` (default) | Executable via system C++ compiler | yes |
| `cpp` | Generated C++ source | yes |
| `manifest` | Canonical actor metadata JSON | no |
| `build-info` | Provenance JSON | yes (text only; parse not required) |
| `ast` | AST dump | yes |
| `graph` | Analysis graph dump | yes |
| `graph-dot` | Graphviz DOT graph | yes |
| `schedule` | Schedule dump | yes |
| `timing-chart` | Mermaid Gantt timing chart | yes |

### 5.2.1 Output Destination Contract

| `--emit` stage | `-o` not given | `-o <path>` given |
|---|---|---|
| `exe` (default) | write `a.out` | write `<path>` |
| `cpp` | write stdout | write `<path>` |
| `manifest` | write stdout | write `<path>` |
| `build-info` | write stdout | write `<path>` |
| `ast` | write stdout | write stdout (unchanged) |
| `graph-dot`, `schedule`, `timing-chart` | write stdout | write stdout (unchanged) |

**Breaking change**: `--emit cpp` without `-o` previously wrote to `a.out`. It now writes to stdout, consistent with all other text-output stages.

### 5.3 Provenance

Provenance tracks "what went in" to a compilation:

- **`source_hash`**: SHA-256 of raw `.pdl` source text (64-char hex)
- **`registry_fingerprint`**: SHA-256 of `Registry::canonical_json()` — compact JSON, decoupled from display formatting (64-char hex)
- **`manifest_schema_version`**: currently 1
- **`compiler_version`**: `env!("CARGO_PKG_VERSION")`

Provenance is computed for every compilation that reads source + registry.

**Stamping in generated C++**: First line of generated C++ includes:

```cpp
// pcc provenance: source_hash=<64hex> registry_fingerprint=<64hex> version=<ver>
```

Machine-parsable, zero runtime cost. Omitted when provenance is not available (unit tests).

**`--emit build-info`** outputs provenance as JSON:

```json
{
  "source_hash": "<64-char hex>",
  "registry_fingerprint": "<64-char hex>",
  "manifest_schema_version": 1,
  "compiler_version": "0.1.2"
}
```

Does NOT require valid parse. Parse-invalid sources produce valid build-info.

### 5.4 Registry and Manifest

**`--emit manifest`**: Scans headers from `-I` / `--actor-path`, outputs canonical `actors.meta.json` (schema v1, actors sorted alphabetically). Does not require `.pdl` source.

**Overlay / precedence rules**:

- **`--actor-meta <manifest>`**: Actor metadata loaded from manifest only (no header scanning for metadata). `-I` / `--actor-path` still collect headers for C++ `-include` flags.
- **Header scanning mode** (no `--actor-meta`): `--actor-path` actors form the base registry; `-I` actors overlay with higher precedence (replace on name conflict).
- **`--emit manifest` + `--actor-meta`**: Usage error (exit code 2).

**Canonical fingerprint**: `Registry::canonical_json()` uses `serde_json::to_string()` (compact, no whitespace). `generate_manifest()` uses `to_string_pretty()` for display. Both share identical sorting/collection logic. The fingerprint is always computed from the compact form — invariant documented in ADR-027.

---

## 6. Diagnostics Contract

All phases emit a shared diagnostic model (`diag.rs`):

- `code` (stable identifier, 54 codes: E0001-E0603, W0001-W0400),
- `level` (`error`/`warning`),
- `message`,
- primary span,
- related spans,
- optional hint/remediation,
- optional cause chain for propagated constraint failures.

Presentation:

- Human-readable diagnostics are default CLI output.
- Machine-readable mode (`--diagnostic-format json`) provides JSONL output.
- Diagnostic stability policy: adding codes is allowed; changing meaning of existing codes requires versioned note.

---

## 7. Failure Modes

Compilation fails when:

- parsing fails,
- resolution fails,
- type/lowering verification fails,
- analysis/scheduling invariants fail,
- backend emission prerequisites are missing,
- external C++ compilation fails for `--emit exe`.

Diagnostic failures identify owning pass and primary span (where available).

Exit codes:

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Compilation error (parse, type, analysis) |
| 2 | Usage error (invalid arguments, incompatible flags) |
| 3 | System error (I/O failure, missing files) |

---

## 8. Performance and Safety

- `pcc` is deterministic for identical input/config.
- `LIR` contains all backend-critical semantic decisions (no backend fallback inference).
- Proof-obligation failures are hard errors.
- Caching may improve latency but must preserve identical outputs/diagnostics.
- Provenance fingerprints are computed from canonical compact JSON, ensuring stability across formatting changes.

---

## 9. Acceptance Tests

### Phase 0 (Spec/ADR Contract Freeze) ✅

- `AST → HIR → THIR → LIR` boundary and ownership documented.
- Pass-manager contract documented.
- Compatibility gate documented.
- ADR set published: ADR-020 (pass manager), ADR-021 (stable IDs), ADR-022 (diagnostics), ADR-023 (compatibility gate).

### Phase 1 (Mechanical Foundations) ✅

- 7 insta snapshot tests lock byte-equivalent output.
- Stable IDs (`CallId`/`DefId`/`TaskId`) thread through all phases.
- Span-as-primary-key eliminated from semantic tables.

### Phase 2 (IR Unification) ✅

- HIR normalization: define expansion, modal normalization.
- ThirContext wrapper: unified query API for downstream phases.
- LIR backend IR: codegen reads LIR only, 48.5% LOC reduction.
- `type_infer` and `lower` consume HIR (not raw AST).

### Phase 3 (Pass Manager) ✅ (partial)

- 9 PassIds, 11 ArtifactIds, dependency resolution.
- `run_pipeline()` with borrow-split `CompilationState`.
- Minimal-pass evaluation for each `--emit` target.
- Invalidation hashing and caching deferred to Phase 3b/3c.

### Phase 4 (Verification Framework) ✅

- `StageCert` trait with HIR/Lower/Schedule/LIR verifiers.
- Verification wired into pipeline runner.
- Regression corpus (`verify_regression.rs`).

### Phase 5 (Diagnostics) ✅

- 54 stable diagnostic codes.
- `--diagnostic-format json` JSONL output.
- Related spans and cause chains for constraint failures.

### Phase 6 (Runtime Shell) ✅

- `pipit_shell.h` runtime library.
- Descriptor-table codegen pattern.
- 12 C++ shell unit tests.

### Phase 7a (Registry Determinism) ✅

- `--emit manifest` produces canonical JSON (no `.pdl` required).
- `--emit build-info` produces provenance JSON.
- Generated C++ includes `// pcc provenance:` comment header.
- Canonical fingerprint via `canonical_json()`, decoupled from display formatting.
- Overlay/precedence rules documented and tested.
- 6 reproducibility tests (byte-identical outputs for same inputs).
- ADR-027 exit criteria all met.

### Phase 7b (CMake Build Integration) ✅

- Manifest-first workflow wired into `examples/CMakeLists.txt`.
- `PIPIT_USE_MANIFEST` option (default ON) with legacy fallback.
- Explicit header inventory with scoped GLOB cross-check.
- CMake dependency chain validated (`test_cmake_regen.sh`).

### Phase 8 (Test Strategy and Migration Hardening) ✅

- IR-level golden tests: HIR, THIR, and LIR insta snapshots (7 tests each, all example PDLs).
- Pipeline equivalence tests: direct call chain vs pass-manager orchestration produce byte-identical C++ output.
- Property-based tests (proptest): parser→HIR roundtrip, widening transitivity/antisymmetry (exhaustive), scheduler invariants.
- Full matrix coverage: all 8 C++ runtime test binaries wired into `cargo test`.

### Deferred

- Phase 3b/3c: Invalidation hashing and artifact cache.
