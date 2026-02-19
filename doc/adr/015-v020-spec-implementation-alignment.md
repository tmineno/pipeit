# ADR-015: v0.2.0 Spec Alignment to Implemented Behavior

## Context

`pipit-lang-spec-v0.2.0.md` had accumulated inconsistencies and several mismatches against the current compiler/runtime behavior.  
A v0.2.0 consistency review and follow-up fixes updated both spec and implementation. This ADR records the authoritative decisions so future changes do not reintroduce drift.

## Decision

### 1. Actor metadata model is header-scan based

- The spec now defines actor metadata acquisition as `ACTOR(...)` declaration scanning from headers (`IN`/`OUT`/`PARAM`/`RUNTIME_PARAM`), not `constexpr` registration-function discovery.
- `pcc` is specified as a constrained text scanner for `ACTOR` macro forms, not a full C++ parser.

### 2. `set scheduler` is removed from v0.2 language surface

- `set scheduler` is no longer a supported option in v0.2.
- The spec now describes the currently implemented scheduling approach directly (fixed PASS-style schedule generation) instead of exposing scheduler selection.

### 3. Probe output target is path-only

- `--probe-output` is defined as `--probe-output <path>` only.
- If users want stderr, they must pass an implementation path such as `/dev/stderr`.
- Network probe transport is out of scope for v0.2.

### 4. Modal/switch semantics are clarified and stabilized

- Initial mode is determined by the **first control-process output** (`ctrl`) at runtime.
- `switch(... ) default <mode>` remains parseable for backward compatibility but is **soft-deprecated** in v0.2:
  - accepted by parser
  - warning emitted by compiler
  - no runtime dispatch effect
- Out-of-range `ctrl` behavior remains **undefined**.

### 5. Runtime errors are fail-fast

- On actor failure or shared-buffer read/write fatal failure, runtime sets global stop and terminates the pipeline.
- Spec no longer describes timeout-chain propagation as normative behavior.

### 6. Global defaults are made internally consistent

- `tick_rate` default is `10kHz` (not `1MHz`).
- `timer_spin` default remains `10000` (10us), and `timer_spin=0` is explicitly “no spin”, not default.

### 7. Modal grammar reflects allowed control source forms

- Modal grammar permits `control_block` optionality, with constraints defining valid `ctrl` supply combinations.
- `switch` source constraints are described normatively in the modal section/constraints rather than relying on ambiguous examples.

### 8. Spec examples are corrected to match current rate math

- Example decimation/rate-matching values were updated to match the implemented SDF/rate assumptions (e.g. receiver logger decimation value).

## Consequences

- `default` in `switch` is now compatibility syntax only; users must not rely on fallback mode behavior.
- Any future scheduler-policy surface needs a new ADR and implementation plan before being added back to spec.
- Probe-output behavior is simpler and deterministic across CLI/runtime.
- Fail-fast semantics make error handling explicit and predictable in generated binaries.

## Alternatives considered

- Keep old spec text and change implementation to match: rejected (larger behavior churn and higher regression risk for v0.2 line).
- Keep `set scheduler` as a placeholder option: rejected (spec surface without runtime effect is misleading).
- Keep `default` as semantic fallback: rejected for v0.2 because runtime dispatch already standardized on numeric `ctrl` switch without fallback selection.

## Exit criteria

- [x] `doc/spec/pipit-lang-spec-v0.2.0.md` reflects all decisions above.
- [x] Compiler emits warning for `switch ... default ...` and ignores it semantically.
- [x] Codegen has no runtime fallback branch tied to `default`.
- [x] Benchmark and test suites pass with aligned semantics.
