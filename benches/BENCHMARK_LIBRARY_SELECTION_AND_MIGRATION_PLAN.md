# Benchmark Library Selection and Migration Plan (for `./benches`)

## Scope

This plan covers benchmark code and scripts under `./benches`.

Out of scope for this document:

- `compiler/benches/compiler_bench.rs` (Criterion-based Rust benchmark)

## Current Problems

- Multiple output formats are mixed: Google Benchmark JSON, custom text (`timer_bench`, `latency_bench`, `pdl`), and `perf` text/JSON.
- Dependencies are partially split: most C++ suites use Google Benchmark, while some suites bypass a benchmark library.
- Result aggregation and regression checks are harder than needed.

## Library Selection

### Candidates considered

1. Google Benchmark
2. Catch2 benchmark
3. ankerl::nanobench

### Decision

Use **Google Benchmark** as the standard benchmark library for `./benches`.

### Why Google Benchmark

- Already adopted by most C++ benchmark suites in this repo.
- Native machine-readable output (`--benchmark_out`, JSON format).
- Supports custom counters, repeated runs, custom statistics, and manual timing.
- Supports multithreaded benchmark patterns used in this repo.
- Actively maintained (latest release is recent).

### Why not the others

- Catch2 benchmark:
  - Strong test framework, but benchmark output standardization typically relies on reporter wiring.
  - Not a clear net win for this repo's existing benchmark-heavy layout.
- nanobench:
  - Good single-header option and JSON template support.
  - Would require broad rewrite from current Google Benchmark code.
  - Lower maintenance activity compared to Google Benchmark.

## Unified Output Format

Adopt **Google Benchmark JSON shape** as the canonical result format for `./benches` outputs.

Top-level structure:

- `context`
- `benchmarks` (array)

For suites that need domain-specific metrics (timer jitter, percentiles, overruns), store them as benchmark counters/fields in benchmark entries.

For non-Google-Benchmark producers (`pdl`, `perf` scripts that output custom data), generate adapter JSON files that conform to the same top-level schema.

## Migration Plan

### Phase 1: Canonical format foundation

1. [x] Add a result adapter utility (shell or small parser binary) under `benches/` to normalize non-GBench outputs to canonical JSON.
2. [x] Update `benches/run_all.sh` so every category writes canonical JSON into `results/`.
3. [x] Keep human-readable logs as optional side artifacts (`*.txt`) but make JSON the source of truth.

Deliverable:

- Every benchmark category under `./benches` emits a canonical JSON artifact.

Implementation notes:

- Added `benches/canonicalize_results.sh` with adapters for:
  - `gbench`, `compiler`, `timer`, `latency`, `pdl`, `perf`
- Updated `benches/run_all.sh` to emit:
  - `compiler.canonical.json`
  - `runtime.canonical.json`
  - `ringbuf.canonical.json`
  - `timer.canonical.json`
  - `thread.canonical.json`
  - `actor.canonical.json`
  - `pdl.canonical.json`
  - `affinity.canonical.json`
  - `memory.canonical.json`
  - `latency.canonical.json`
  - `perf.canonical.json`

### Phase 2: Migrate custom C++ suites to Google Benchmark

1. [x] Convert `benches/timer_bench.cpp` to Google Benchmark harness.
2. [x] Convert `benches/latency_bench.cpp` to Google Benchmark harness.
3. [x] Use:
   - `UseManualTime` for real-time constrained sections.
   - custom counters/statistics for p90/p99/p999, overruns, and ratio metrics.
4. [x] Keep benchmark names stable and explicit (`Timer/*`, `Latency/*`) for comparability.

Deliverable:

- `timer_bench` and `latency_bench` emit native Google Benchmark JSON directly.

Implementation notes:

- `benches/timer_bench.cpp` migrated to Google Benchmark with:
  - frequency sweep
  - jitter histogram counters
  - overrun recovery
  - wakeup latency
  - spin-threshold comparison
  - batch-vs-single comparison
  - high-frequency batched sweep
- `benches/latency_bench.cpp` migrated to Google Benchmark with:
  - per-actor latency distributions
  - timer vs work ratio
  - ring-buffer vs compute budget
  - wakeup latency
  - end-to-end stage budget
  - batched and high-frequency timer/work analysis
- `benches/run_all.sh` now runs both via `build_and_run_gbench` and canonicalizes from `*.json`.

### Phase 3: PDL and perf alignment

1. [x] `pdl` benchmark path:
   - Extract `[stats]` lines and map to canonical JSON benchmark entries.
2. [x] `perf` benchmark path:
   - Keep raw `perf` outputs, plus emit canonical JSON summary entries.
3. [x] Ensure naming convention consistency:
   - `<suite>/<scenario>/<variant>`

Deliverable:

- `pdl` and `perf` results are queryable with the same JSON processing pipeline.

Implementation notes:

- `benches/canonicalize_results.sh` now emits:
  - `pdl/<program>/task:<task_name>`
  - `pdl/<program>/buffer:<buffer_name>`
- `perf` canonicalization now aggregates both:
  - `perf_*.txt`
  - `perf_*.json`
- `perf` entries are normalized as:
  - `perf/<scenario>/<variant>`
  - plus explicit fields: `suite`, `scenario`, `variant`, `source_file`, `source_format`

### Phase 4: Validation and regression workflow

1. [x] Add schema validation step for all produced JSON files.
2. [x] Add a comparison tool/script for baseline vs current runs.
3. [x] Integrate into CI (at least smoke + nightly perf lane).

Deliverable:

- Reproducible machine-readable performance tracking across all `./benches` categories.

Implementation notes:

- Added `benches/schema/canonical-benchmark.schema.json` (canonical schema reference).
- Added `benches/validate_canonical_results.sh`:
  - validates canonical JSON core shape and suite-specific naming constraints.
- Added `benches/compare_canonical_results.sh`:
  - compares baseline vs current canonical results with threshold-based regression tagging.
  - emits `baseline_comparison.md`.
- Updated `benches/run_all.sh` with Phase 4 options:
  - `--validate`, `--validate-schema`
  - `--compare-baseline-dir`, `--compare-output`, `--compare-threshold-pct`
  - `--compare-allow-missing-baseline`, `--compare-no-fail-on-regression`
- Integrated CI:
  - `.github/workflows/ci.yml`: benchmark smoke lane (`runtime` + validation/report).
  - `.github/workflows/bench-nightly.yml`: nightly perf lane with validation + baseline comparison.
  - `benches/baselines/nightly/.gitkeep`: baseline directory seed.

## File-Level Impact

- `benches/run_all.sh`
- `benches/timer_bench.cpp`
- `benches/latency_bench.cpp`
- `benches/perf/perf_common.sh` (and possibly per-script outputs)
- `benches/README.md`
- `benches/canonicalize_results.sh` (implemented in Phase 1)
- `benches/validate_canonical_results.sh` (Phase 4)
- `benches/compare_canonical_results.sh` (Phase 4)
- `benches/schema/canonical-benchmark.schema.json` (Phase 4)
- `.github/workflows/ci.yml` (Phase 4 smoke integration)
- `.github/workflows/bench-nightly.yml` (Phase 4 nightly integration)
- `benches/baselines/nightly/.gitkeep` (Phase 4 baseline seed)

## Acceptance Criteria

- Running `./benches/run_all.sh` produces canonical JSON for all selected categories.
- No category is JSON-missing by default.
- Existing benchmark intent (what is measured) remains unchanged after migration.
- Benchmark names and metrics are stable enough for automated diff/regression checks.
