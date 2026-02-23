# Feature: pcc — Pipit Compiler Collection

Version: 0.4.0 (Draft)

## 1. Goal

`pcc` compiles Pipit Definition Language (`.pdl`) programs into generated C++ or executables while preserving static SDF guarantees and deterministic behavior.

For v0.4.0, the primary goal is architectural: define explicit IR boundaries, pass ownership, and pass-manager contracts so downstream phases consume a single typed/lowered source of truth.

Refer to [pipit-lang-spec-v0.4.0](pipit-lang-spec-v0.4.0.md) for language semantics. This document specifies compiler tool behavior and architecture contracts.

## 2. Non-goals

- No mandatory incremental/watch mode.
- No mandatory distributed build cache.
- No C++ AST parsing of actor implementation bodies.
- No protocol-level reliability guarantees beyond each transport spec (`PPKT` / `PSHM`).

---

## 3. Compatibility Gate

v0.4.0 adopts a compatibility gate:

- Default behavior keeps v0.3.x language and CLI compatibility unless v0.4.0 language spec deltas are explicitly enabled.
- Any breaking behavior requires all of:
  - explicit spec delta in this file (or successor spec),
  - dedicated ADR with migration reasoning,
  - release-note entry with impact and migration path.

Compatibility gate scope includes:

- language parsing/typing behavior,
- `pcc` CLI options and defaults,
- output-stage semantics (`--emit`),
- runtime option behavior in generated binaries (`--duration`, `--param`, `--bind`, `--probe`, `--probe-output`, `--stats`).

---

## 4. Architecture Contract

### 4.1 IR Boundaries

v0.4.0 defines four explicit compiler IR stages:

1. `AST`:
   - Parsed syntax with spans.
   - No semantic resolution.
1. `HIR` (resolved/normalized):
   - Name resolution complete.
   - Structural normalization (define expansion policy, task/mode normalization, explicit tap/buffer semantics).
1. `THIR` (typed/lowered):
   - Type inference and monomorphization complete.
   - Safe implicit widening materialized as explicit nodes.
   - Proof obligations validated (existing L1-L5 minimum).
1. `LIR` (scheduled/backend-ready):
   - Scheduling, buffer layout, concrete actor instantiation, and backend-required facts finalized.
   - Backend is syntax-directed from `LIR` (no semantic re-inference).

Canonical phase flow:

```
AST -> HIR -> THIR -> LIR -> C++ generation
```

### 4.2 Pass Ownership

| Pass | Input IR | Output IR/Artifact | Owns |
|---|---|---|---|
| Parse | source text | `AST` | grammar/lexing |
| Resolve + Normalize | `AST`, registry | `HIR` | symbols, scope, normalization |
| Type Infer + Mono + Lower Verify | `HIR`, registry | `THIR` | typing, monomorphization, widening safety |
| Graph/Analyze/Schedule | `THIR` | `LIR` | graph facts, rates, buffers, schedule |
| Bind Infer + Contract Check | `LIR` | `BindInterface` | bind direction/contract inference, stable-id assignment |
| Codegen | `LIR`, `BindInterface` | C++ source | serialization only |

Rules:

- A pass may consume only declared artifacts.
- A pass may not re-infer semantics owned by earlier passes.
- Cross-pass data sharing is via artifacts, not hidden side channels.

### 4.3 Pass Manager Contract

Each pass declares:

- `inputs`: required artifacts and config fields,
- `outputs`: produced artifacts,
- `invariants`: pre/post conditions,
- `invalidation_key`: deterministic hash inputs used for cache validity.

`--emit` targets resolve required artifacts via dependency graph and evaluate the minimal pass subset.

### 4.4 Artifact/Caching Contract

- Artifact keys are deterministic across machines for equal inputs/config.
- Registry provenance (manifest/header hash set, schema version) participates in invalidation.
- Cache miss or verification failure falls back to recompute.
- Cache behavior must not change observable compiler semantics.
- `BindInterface` (backing optional interface manifest output) is a first-class artifact and participates in deterministic invalidation.

---

## 5. Inputs and Outputs

### 5.1 Inputs

- `.pdl` source (required),
- actor metadata:
  - preferred: `--actor-meta` manifest (`actors.meta.json`),
  - fallback: header scanning via `-I` / `--include` / `--actor-path`,
- compilation config (`--emit`, `--cc`, `--cflags`, `--release`, etc.),
- bind endpoint overrides (`--bind <name>=<endpoint>`, optional, repeatable).
- optional interface manifest output path (`--interface-out <path>`).

### 5.2 Outputs

- `--emit ast`: AST dump,
- `--emit graph`: analysis graph dump,
- `--emit graph-dot`: DOT graph,
- `--emit schedule`: schedule dump,
- `--emit timing-chart`: Mermaid timing chart,
- `--emit cpp`: generated C++,
- `--emit interface` (optional): bind contract manifest (`stable_id`, direction, contract, endpoint),
- default `--emit exe`: executable via system C++ compiler.

Phase 0 expects stage outputs to remain behaviorally compatible with v0.3.x unless explicitly versioned as breaking.

### 5.3 Bind Compilation Contract

For the `bind` specification (`pipit-lang-spec-v0.4.0`, section 5.11), `pcc` must satisfy the following.

1. **direction inference**
   - If `-> name` exists, infer `out`.
   - If `-> name` does not exist and only `@name` exists, infer `in`.
   - Otherwise, emit a compile-time error.

1. **contract inference**
   - `dtype` / `shape`: determined from buffer type information in LIR.
   - `rate_hz`: determined from `tokens_per_iter * task_rate_hz` on writer/reader sides.
   - For an `in` bind, if required rates from multiple readers do not match, emit an error.

1. **stable_id assignment**
   - `stable_id` is generated deterministically from semantic IDs (task/node/edge lineage), not span/name text.
   - It must remain stable for identical input and compiler configuration (deterministic).

1. **endpoint validation**
   - Validate `udp` / `unix_dgram` endpoint arguments against the PPKT spec.
   - Validate `shm` endpoint arguments against the PSHM spec.

1. **manifest emission**
   - Emit an interface manifest when `--emit interface` or `--interface-out <path>` is specified.
   - When emitted, the manifest must contain bind contract information consistent with generated C++.

`pcc` MUST NOT change the SDF schedule as a side effect of bind inference/validation.

---

## 6. Diagnostics Contract

All phases emit a shared diagnostic model:

- `code` (stable identifier),
- `level` (`error`/`warning`),
- `message`,
- primary span,
- related spans,
- optional hint/remediation,
- optional cause chain for propagated constraint failures.

Presentation requirements:

- human-readable diagnostics remain default CLI output,
- machine-readable mode (`json`) provides structured diagnostics for tooling,
- diagnostic stability policy: adding codes is allowed; changing meaning of existing codes requires versioned note.
- bind-related diagnostics include at least:
  - direction inference failure,
  - contract ambiguity/mismatch,
  - duplicate bind target,
  - unsupported endpoint option/value.

---

## 7. Failure Modes

Compilation fails when:

- parsing fails,
- resolution fails,
- type/lowering verification fails,
- analysis/scheduling invariants fail,
- bind inference/contract validation fails,
- backend emission prerequisites are missing,
- external C++ compilation fails for `--emit exe`.

Diagnostic failures must identify owning pass and primary span (where available).

---

## 8. Performance and Safety

- `pcc` remains deterministic for identical input/config.
- `LIR` must contain all backend-critical semantic decisions (no backend fallback inference).
- Proof-obligation failures are hard errors.
- Caching may improve latency but must preserve identical outputs/diagnostics.

---

## 9. Acceptance Tests (Phase 0)

- `AST -> HIR -> THIR -> LIR` boundary and ownership are documented and internally consistent.
- Pass-manager contract (inputs/outputs/invalidation/invariants) is documented.
- Compatibility gate is documented and explicit.
- bind inference contract is documented (`direction`, `dtype/shape/rate`, deterministic `stable_id`).
- optional interface manifest artifact contract is documented.
- ADR set for Phase 0 architecture decisions is published:
  - `ADR-020` pass-manager artifact model,
  - `ADR-021` stable semantic IDs,
  - `ADR-022` diagnostics model,
  - `ADR-023` backward-compatibility gate.
