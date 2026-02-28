# ADR-033: Manifest-Required Inputs (Breaking)

## Context

Since v0.4.2, `pcc` supports two actor metadata paths:

1. **Header scanning** (`-I`, `--actor-path`): scan C++ headers at compile time.
2. **Manifest** (`--actor-meta actors.meta.json`): load pre-generated metadata.

Header scanning at compile time is non-hermetic — the same headers may produce different results across machines due to filesystem layout, include order, and preprocessor conditionals. ADR-027 introduced `--emit manifest` to decouple metadata generation from compilation, but both paths remained available for all stages.

v0.4.4 replaces the text scanner with preprocessor-based extraction (ADR-032). To complete the hermetic build model, compilation paths should consume only pre-generated manifests.

## Decision

### `--actor-meta` is required for compilation stages

The following `--emit` stages now require `--actor-meta`:

| Stage | Requires `--actor-meta` | Why |
|-------|-------------------------|-----|
| `manifest` | No | This stage _generates_ the manifest |
| `ast` | No | Parse-only dump, no registry needed |
| `build-info` | Yes | Needs registry for fingerprint |
| `cpp`, `exe` | Yes | Full compilation pipeline |
| `graph`, `graph-dot` | Yes | Graph construction needs registry |
| `schedule`, `timing-chart` | Yes | Schedule needs registry |

Missing `--actor-meta` on a required stage produces **E0700** (exit code 2).

### E0700 diagnostic

E0700 is stage-aware and respects `--diagnostic-format`:

**Human format:**

```
error[E0700]: --actor-meta is required for --emit <stage>
  hint: generate a manifest first:
        pcc --emit manifest -I <include> -o actors.meta.json
        then: pcc source.pdl --actor-meta actors.meta.json --emit <stage>
```

**JSON format:**

```json
{
  "kind": "usage",
  "level": "error",
  "code": "E0700",
  "message": "--actor-meta is required for --emit <stage>",
  "span": {"start": 0, "end": 0},
  "hint": "generate a manifest first: pcc --emit manifest -I <include> -o actors.meta.json",
  "related_spans": [],
  "cause_chain": []
}
```

### Centralized usage-error emitter

Existing usage errors (`main.rs:101`, `main.rs:120`) are ad-hoc `eprintln!` calls that ignore `--diagnostic-format`. A new `emit_usage_error()` helper centralizes this:

```rust
fn emit_usage_error(cli: &Cli, code: Option<DiagCode>, message: &str, hint: Option<&str>) -> !
```

All existing ad-hoc usage-error sites are migrated to this helper.

### Migration path

**Before (v0.4.3):**

```bash
pcc source.pdl -I runtime/libpipit/include --emit cpp -o out.cpp
```

**After (v0.4.4):**

```bash
# Step 1: generate manifest (once, or as build step)
pcc --emit manifest -I runtime/libpipit/include -o actors.meta.json

# Step 2: compile using manifest
pcc source.pdl --actor-meta actors.meta.json -I runtime/libpipit/include --emit cpp -o out.cpp
```

The `-I` flags remain on the compile step — they provide header paths for C++ `#include` directives in generated code, but are no longer used for metadata extraction.

## Consequences

- **Breaking change**: all compilation commands must now include `--actor-meta`.
- **Hermetic builds enforced**: metadata source is always a stable, pre-generated artifact.
- **Two-step workflow required**: users must generate a manifest before compiling.
- **`ast` stage preserved**: parse-only debugging does not require a manifest.
- **Diagnostic consistency**: all usage errors now respect `--diagnostic-format`.

## Alternatives

- **Keep optional `--actor-meta`**: Rejected — perpetuates non-hermetic header scanning at compile time, undermining reproducibility.
- **Auto-generate manifest inline when `--actor-meta` absent**: Rejected — hides a hermetic/non-hermetic choice behind implicit behavior, making build reproducibility opaque.
- **Require manifest for `ast` stage too**: Rejected — `ast` is a parse-only debug dump that has no dependency on actor metadata.

## Exit criteria

- [ ] E0700 emitted for `--emit cpp|exe|build-info|graph|graph-dot|schedule|timing-chart` without `--actor-meta`
- [ ] E0700 respects `--diagnostic-format` (human and JSON)
- [ ] `--emit ast` succeeds without `--actor-meta`
- [ ] `--emit manifest` succeeds without `--actor-meta` (generates it)
- [ ] `emit_usage_error()` helper centralizes all usage-error output
- [ ] Existing ad-hoc `eprintln!` usage-error sites migrated
- [ ] All test files updated with manifest pre-gen steps
- [ ] Release note documents breaking change and migration path (ADR-023 requirement)
- [ ] All existing tests remain green
