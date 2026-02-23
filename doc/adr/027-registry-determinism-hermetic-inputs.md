# ADR-027: Registry Determinism and Hermetic Build Inputs

## Context

The Pipit compiler accepts actor metadata via two paths: header scanning (`-I`, `--actor-path`) and manifest (`--actor-meta actors.meta.json`). Header scanning depends on filesystem layout and C++ header content at scan time — the same headers may be discovered in different order on different machines, and header content changes are invisible to downstream build caching.

For CI reproducibility and deterministic builds, three problems must be solved:

1. **Manifest-first workflow**: Header scanning should be an explicit *generation* step, producing a stable `actors.meta.json` that the compilation step consumes hermetically.
2. **Provenance auditing**: Given a compiled artifact, it must be possible to determine what source and registry were used.
3. **Overlay ambiguity**: The precedence rules for combining `--actor-meta`, `-I`, and `--actor-path` must be documented and tested.

## Decision

### `--emit manifest`: Explicit metadata generation

Add `--emit manifest` as a new emit stage that scans headers from `-I` / `--actor-path` and outputs canonical `actors.meta.json`. This stage does NOT require a `.pdl` source file.

Combining `--emit manifest` with `--actor-meta` is a **usage error** (exit code 2) — it would mean "load a manifest and re-emit it", which is pointless.

### `--emit build-info`: Provenance auditing

Add `--emit build-info` as a new emit stage that outputs a provenance JSON document:

```json
{
  "source_hash": "<64-char hex SHA-256>",
  "registry_fingerprint": "<64-char hex SHA-256>",
  "manifest_schema_version": 1,
  "compiler_version": "0.1.2"
}
```

This stage requires source **text** (for hashing) and registry, but does **NOT** require a valid parse. Parse-invalid sources still produce valid build-info — provenance is about "what went in", not "does it compile".

Execution order: `manifest early-exit → read source → build-info early-exit → parse → ast early-exit → pipeline`

### Provenance stamping in generated C++

Generated C++ includes a provenance comment header as the first line:

```cpp
// pcc provenance: source_hash=<64hex> registry_fingerprint=<64hex> version=0.1.2
```

The comment is machine-parsable and has zero runtime cost.

### Canonical fingerprint (not display formatting)

`generate_manifest()` uses `serde_json::to_string_pretty()` for human-readable output. If the fingerprint were computed from this pretty-printed form, any formatting change (indentation, whitespace) would break the hash even though the data is identical.

The fingerprint is computed from `Registry::canonical_json()` — a separate method that uses `serde_json::to_string()` (compact, no whitespace). Both methods share the same sorting/collection logic (actors sorted alphabetically by name), differing only in serialization:

- `generate_manifest()`: pretty-printed, for display and `--emit manifest`
- `canonical_json()`: compact, for fingerprint computation only

**Invariant**: The registry fingerprint is SHA-256 of compact canonical JSON, never of any pretty-printed form.

### Output destination contract

Non-binary emit stages (`cpp`, `manifest`, `build-info`) default to **stdout** when `-o` is not specified. Only `--emit exe` defaults to writing a file (`a.out`). This aligns with existing text-output stages (`graph-dot`, `schedule`, `timing-chart`) which already write to stdout.

**Breaking change**: `--emit cpp` without `-o` previously wrote to `a.out` (the default). It now writes to stdout.

### Overlay / precedence rules

When `--actor-meta <manifest>` is provided:

- Actor metadata is loaded from manifest **only** (no header scanning for metadata)
- `-I` / `--actor-path` only collect headers for C++ `-include` flags
- `--emit manifest` + `--actor-meta` = usage error (exit code 2)

When `--actor-meta` is NOT provided (header scanning mode):

- `--actor-path` actors loaded first (base registry)
- `-I` actors overlay with higher precedence (replace on conflict)

## Consequences

- **Hermetic builds**: `--emit manifest` + `--actor-meta` pipeline eliminates filesystem-dependent header scanning from the compilation step
- **Provenance auditing**: `--emit build-info` and C++ comment header enable tracing artifacts back to their inputs
- **Fingerprint stability**: Canonical compact JSON ensures fingerprints survive display formatting changes
- **`--emit cpp` breaking change**: Users relying on `--emit cpp` without `-o` writing to `a.out` must now specify `-o <path>` explicitly

## Alternatives

- **Hash the pretty-printed manifest directly**: Rejected — coupling fingerprint to display formatting is fragile. Any `serde_json` option change would invalidate all cached fingerprints.
- **Binary serialization for fingerprint**: Rejected — more complex to implement, harder to debug, and Serde's compact JSON is already well-defined and stable.
- **Embed provenance in C++ as a `constexpr`**: Rejected — adds runtime data without benefit. A comment is zero-cost and machine-parsable.
- **`--provenance` as a separate flag**: Rejected — `--emit build-info` follows the existing `--emit` pattern consistently.
- **Require valid parse for `--emit build-info`**: Rejected — provenance is about inputs ("what went in"), not compilation success. Users need to inspect provenance even for failing builds.

## Exit criteria

- [x] `--emit manifest` produces canonical JSON from header scanning (no `.pdl` required)
- [x] `--emit build-info` produces provenance JSON (source_hash, registry_fingerprint, manifest_schema_version, compiler_version)
- [x] `--emit build-info` succeeds with parse-invalid source
- [x] Generated C++ includes `// pcc provenance:` comment header
- [x] `canonical_json()` produces compact JSON, decoupled from `generate_manifest()` formatting
- [x] Overlay/precedence rules documented and tested
- [x] `--emit manifest` + `--actor-meta` = usage error (exit code 2)
- [x] Reproducibility tests pass (byte-identical outputs for same inputs)
- [x] All existing tests remain green
- [x] Phase 7b (CMake integration) documented as deferred
