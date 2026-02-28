# ADR-032: Preprocessor Record Manifest Extraction

## Context

`pcc --emit manifest` currently extracts actor metadata via a text scanner in `registry.rs` (~440 lines) that pattern-matches raw C++ header text. This approach cannot resolve `#include`, `#ifdef`, or preprocessor conditionals — actors behind guards are invisible, and headers must be self-contained. Macro-generated or conditionally compiled actors are silently missed.

Two replacement strategies were evaluated:

1. **libTooling-based extractor** (`pipit-reggen`): a separate C++ binary using Clang's AST tooling for full type-aware extraction.
2. **Preprocessor record approach**: redefine the `ACTOR` macro to emit structured text records, pipe through the C++ preprocessor (`-E -P`), and parse the output in Rust.

Option 2 was selected for lower deployment complexity and maintenance surface while solving the core include/ifdef weakness.

## Decision

### Probe translation unit

`pcc` constructs a probe C++ source in memory that:

1. Includes `pipit.h` first (activating type aliases and the include guard).
2. Undefines and redefines `ACTOR`, `IN`, `OUT`, `PARAM`, `RUNTIME_PARAM`, and `_PIPIT_FIRST`.
3. Redefines `ACTOR` to emit `PIPIT_REC_V1(__FILE__, __LINE__, #name, #in_spec, #out_spec, #__VA_ARGS__)`.
4. Includes all discovered actor headers (absolute paths from `-I` and `--actor-path`).

Self-referential macro trick (`#define IN(type, count) IN(type, count)`) prevents expansion via the blue-paint rule, preserving text for stringification.

### Preprocessor invocation

```
<cc> -E -P -x c++ -std=c++20 -
  -I <runtime/libpipit/include>
  -I <runtime/libpipit/include/third_party>
  -I <user dirs> ...
```

- `-std=c++20` is always pinned (pcc requires C++20 for codegen). `--cflags` does NOT participate in PP extraction.
- Runtime include root and `third_party/` subdirectory are auto-added (same logic as `--emit exe` path).
- `--cc` flag selects the compiler (default: `clang++`).

### Record format

After preprocessing, each actor produces:

```
PIPIT_REC_V1("file.h", 42, "name", "IN(float, 1)", "OUT(float, 1)", "PARAM(float, x)")
```

For template actors, the `template` keyword survives preprocessing:

```
template <typename T>
PIPIT_REC_V1("poly.h", 5, "poly_scale", "IN(T, 1)", "OUT(T, 1)", "RUNTIME_PARAM(T, scale)")
```

### Record parser

`parse_record_fields()` extracts 6 typed fields: `(file: &str, line: u32, name: &str, in_spec: &str, out_spec: &str, params: &str)`.

Template parameters are captured by backward-scanning from each `PIPIT_REC_V1` position for `template <...>` — reusing the existing `extract_template_params()` function on preprocessor-resolved output.

### Overlay precedence

The probe TU includes ALL headers from both `-I` and `--actor-path`. After extraction, records are split by `__FILE__` path into per-source-class registries. Same-group duplicates are errors (`RegistryError::DuplicateActor`), cross-group conflicts are resolved by `-I` precedence (`overlay_from()`). This preserves existing behavior.

### Error model

| Condition | Exit code | Error type |
|-----------|-----------|------------|
| Compiler not found / launch failure | 3 | `RegistryError::PreprocessorError` (new variant) |
| Preprocessing failure (bad include) | 3 | `RegistryError::PreprocessorError` |
| Record parse failure | 1 | `RegistryError::ParseError` (existing) |

### Functions replaced

| New | Replaces |
|-----|----------|
| `scan_actors_pp()` | `scan_actors()` |
| `build_probe_tu()` | — |
| `invoke_preprocessor()` | — |
| `parse_pp_records()` | `parse_actor_macro()` |
| `parse_record_fields()` | `extract_balanced()` |
| `unescape_string_literal()` | — |

Functions deleted: `scan_actors()`, `strip_comments()`, `extract_balanced()`, `parse_actor_macro()`.

Functions reused: `parse_port_spec()`, `parse_port_shape()`, `parse_param_spec()`, `split_param_specs()`, `split_top_level_commas()`, `parse_pipit_type()`, `parse_param_type()`, `parse_token_count()`, `extract_template_params()`.

## Consequences

- Preprocessor resolves includes/ifdefs correctly — actors behind guards become visible.
- `pcc` remains Rust-only with no new binaries or LLVM dev dependencies.
- Dependency on a C++ preprocessor in `PATH` for `--emit manifest` (same `--cc` flag already required for `--emit exe`).
- Text scanner (~440 lines) is fully replaced and deleted.
- Pluggable architecture preserved: a libTooling backend can be introduced later if stricter type resolution becomes necessary.

## Alternatives

- **libTooling extractor (`pipit-reggen`)**: Rejected for v0.4.4 — requires LLVM dev libraries, separate binary distribution, CI provisioning in multiple jobs, and binary resolution contract. Higher deployment and maintenance cost for marginal type-fidelity benefit at current schema scope.
- **Keep text scanner with incremental fixes**: Rejected — fundamental inability to resolve preprocessor conditionals makes incremental improvement insufficient.
- **Mirror `--cflags` in PP extraction**: Rejected — actor headers are expected to compile under C++20 (pcc's codegen requirement). Allowing a different standard introduces divergence risk between extraction and compilation.

## Exit criteria

- [ ] `scan_actors_pp()` produces identical `ActorMeta` output as `scan_actors()` for all stdlib + example actors (golden parity test)
- [ ] `--emit manifest` uses PP extraction path
- [ ] Template actors are correctly captured with type parameters
- [ ] Overlay precedence preserved (same-group duplicate = error, cross-group = `-I` wins)
- [ ] `RegistryError::PreprocessorError` variant handles compiler-not-found and preprocessing failures (exit 3)
- [ ] Text scanner dead code (`scan_actors`, `strip_comments`, `extract_balanced`, `parse_actor_macro`) is removed
- [ ] Determinism test: repeated `--emit manifest` output is byte-identical
- [ ] All existing tests remain green
