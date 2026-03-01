# Pipit Compiler Profiling Protocol

## Purpose

Defines a reproducible, quantitative procedure for measuring compiler
performance before/after optimization work. Used for release gate
verification (regression/improvement gates).

## Fixed Corpus

| Scenario | Description | Nodes | Tasks |
| --- | --- | --- | --- |
| simple | stdin \| abs \| stdout (1 kHz) | 3 | 1 |
| multitask | 10 kHz capture + 1 kHz drain (multirate) | ~8 | 2 |
| complex | 20 kHz producer + 2 kHz consumer with FIR | ~10 | 2 |
| modal | Adaptive control flow with 2 modes | ~12 | 1 |

Corpus PDL definitions are embedded in `benches/compiler_bench.rs`.

## Measurement Settings

| Parameter | Value | Rationale |
| --- | --- | --- |
| CPU pinning | `taskset -c 1` | Eliminate scheduler noise |
| Criterion sample_size | 40 | Sufficient for stable median |
| Criterion measurement_time | 1.0 s | Capture enough iterations |
| Criterion warm_up_time | 0.2 s | Warm instruction cache |
| Independent invocations (N) | 10 | Required by v0.4.6 M3 (N>=10) |

## Procedure

1. Pin CPU governor to `performance` if possible
2. Run `compiler_bench_stable.sh` N=10 times on **baseline** ref
3. Run N=10 times on **current** HEAD
4. Extract per-benchmark median from each invocation
5. Compute across the N invocations: **median**, **p90**, **stddev**

## Targeted Phases

- `kpi/full_compile_latency/{simple,multitask,complex,modal}`
- `kpi/phase_latency/{parse,resolve,graph,analyze,schedule}/{complex}`
- `kpi/phase_latency/{build_thir_context,build_lir,emit_cpp}/{complex}`

## Gate Criteria

### Regression gate

Fail if any targeted phase regresses by >5% median vs baseline:

```text
(current_median - baseline_median) / baseline_median > 0.05  =>  FAIL
```

### Improvement gate

Require measurable median reduction on at least 2 representative
workloads out of:

- `full_compile_latency/complex`
- `full_compile_latency/modal`
- `phase_latency/schedule/complex`
- `phase_latency/codegen/complex`

## Output Artifacts

| Artifact | Format | Location |
| --- | --- | --- |
| Per-invocation raw data | Criterion JSON | `target/stable_bench/criterion/` |
| Aggregated comparison | CSV | `doc/performance/v0XX_ab_comparison.csv` |
| Aggregated comparison | JSON | `doc/performance/v0XX_ab_comparison.json` |
| Flamegraphs | SVG | `doc/performance/flamegraphs/v0XX/` |
| Gate verdict | text in report | `doc/performance/<timestamp>-bench.md` |
