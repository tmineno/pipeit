# Commit Characterization

- ID: `382c9aed49bd` (source: `staged-diff`)
- HEAD: `2615a5d`
- Branch: `main`
- Generated: 2026-02-21T21:12:22+09:00
- Previous report: `810ed6bec1f9-characterize.md`

## Commands

- Runtime/E2E: `/home/tmineno/vibecoding/pipeit/benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir /home/tmineno/vibecoding/pipeit/tmp/performance/382c9aed49bd-20260221-211145/runtime`
- Compile (<=30s): `timeout 30s cargo bench --manifest-path /home/tmineno/vibecoding/pipeit/compiler/Cargo.toml --bench compiler_bench -- kpi/full_compile_latency --sample-size 10 --measurement-time 0.10 --warm-up-time 0.05 --output-format bencher`

## Status

| Section | Status | Note |
|---|---|---|
| runtime/e2e | pass | `/home/tmineno/vibecoding/pipeit/tmp/performance/382c9aed49bd-20260221-211145/runtime.log` |
| compile | pass | wall=1901.934ms, log=`/home/tmineno/vibecoding/pipeit/tmp/performance/382c9aed49bd-20260221-211145/compile.log` |

## Full Compile Latency

| Scenario | ns/iter | Delta vs prev |
|---|---:|---:|
| simple | 10222 | +211 (+2.11%) |
| multitask | 29055 | -810 (-2.71%) |
| complex | 32133 | -2474 (-7.15%) |
| modal | 36720 | +825 (+2.30%) |

## Runtime Deadline Miss Rate

| Clock | miss_rate_pct | Delta vs prev |
|---|---:|---:|
| 1kHz | 0E-16 | +0 |
| 10kHz | 2.3666666666666667 | -0.7 (-22.83%) |
| 48kHz | 75.433333333333337 | -0.1 (-0.13%) |

## Ring Buffer Contention

| Readers | reader_tokens_per_sec | Delta vs prev |
|---:|---:|---:|
| 1 | 774320661.65807974 | +1.54317e+08 (+24.89%) |
| 2 | 937358045.25419307 | +1.66069e+07 (+1.80%) |
| 4 | 922007977.92284489 | -1.16848e+08 (-11.25%) |
| 8 | 670314476.08530533 | -544205 (-0.08%) |

## E2E Throughput

| Benchmark | samples_per_sec | Delta vs prev |
|---|---:|---:|
| Pipeline/64 | 15317334072.051123 | +1.03152e+09 (+7.22%) |
| Pipeline/256 | 17965782399.540218 | +2.94093e+08 (+1.66%) |
| Pipeline/1024 | 16213741887.600349 | +2.76604e+08 (+1.74%) |
| Socket/64 rx | NA | - |
| Socket/256 rx | NA | - |
| Socket/1024 rx | NA | - |

- Socket benchmark errors: `3`
- Socket error message: Failed to bind receiver on localhost:19876

## KPI Snapshot (Stable Keys)

| Key | Value | Unit | Delta vs prev |
|---|---:|---|---:|
| `compile.full.simple_ns_per_iter` | 10222 | ns/iter | +211 (+2.11%) |
| `compile.full.multitask_ns_per_iter` | 29055 | ns/iter | -810 (-2.71%) |
| `compile.full.complex_ns_per_iter` | 32133 | ns/iter | -2474 (-7.15%) |
| `compile.full.modal_ns_per_iter` | 36720 | ns/iter | +825 (+2.30%) |
| `compile.full.wall_ms` | 1901.934 | ms | +23.475 (+1.25%) |
| `compile.full.timed_out` | 0 | bool(0/1) | +0 |
| `runtime.thread.deadline_1khz_miss_rate_pct` | 0E-16 | pct | +0 |
| `runtime.thread.deadline_10khz_miss_rate_pct` | 2.3666666666666667 | pct | -0.7 (-22.83%) |
| `runtime.thread.deadline_48khz_miss_rate_pct` | 75.433333333333337 | pct | -0.1 (-0.13%) |
| `runtime.timer.freq_10khz_p99_ns` | 105922.00000000000 | ns | -6574 (-5.84%) |
| `runtime.timer.freq_10khz_overruns` | 37.000000000000000 | count | -17 (-31.48%) |
| `runtime.timer.adaptive_auto_p99_ns` | 23430.000000000000 | ns | -2130 (-8.33%) |
| `runtime.ringbuf.contention_1reader_tokens_per_sec` | 774320661.65807974 | items/s | +1.54317e+08 (+24.89%) |
| `runtime.ringbuf.contention_2readers_tokens_per_sec` | 937358045.25419307 | items/s | +1.66069e+07 (+1.80%) |
| `runtime.ringbuf.contention_4readers_tokens_per_sec` | 922007977.92284489 | items/s | -1.16848e+08 (-11.25%) |
| `runtime.ringbuf.contention_8readers_tokens_per_sec` | 670314476.08530533 | items/s | -544205 (-0.08%) |
| `runtime.ringbuf.contention_4readers_read_fail_pct` | 96.119490773221571 | pct | -0.0021882 (-0.00%) |
| `e2e.pipeline_64_samples_per_sec` | 15317334072.051123 | samples/s | +1.03152e+09 (+7.22%) |
| `e2e.pipeline_256_samples_per_sec` | 17965782399.540218 | samples/s | +2.94093e+08 (+1.66%) |
| `e2e.pipeline_1024_samples_per_sec` | 16213741887.600349 | samples/s | +2.76604e+08 (+1.74%) |
| `e2e.socket_64_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_256_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_1024_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_error_count` | 3 | count | +0 (+0.00%) |

## Artifacts

- Runtime log: `/home/tmineno/vibecoding/pipeit/tmp/performance/382c9aed49bd-20260221-211145/runtime.log`
- Compile log: `/home/tmineno/vibecoding/pipeit/tmp/performance/382c9aed49bd-20260221-211145/compile.log`
- Runtime JSON dir: `/home/tmineno/vibecoding/pipeit/tmp/performance/382c9aed49bd-20260221-211145/runtime`

## Machine Readable Metrics

<!-- PIPIT_METRICS_BEGIN -->
compile.full.simple_ns_per_iter|10222|ns/iter
compile.full.multitask_ns_per_iter|29055|ns/iter
compile.full.complex_ns_per_iter|32133|ns/iter
compile.full.modal_ns_per_iter|36720|ns/iter
compile.full.wall_ms|1901.934|ms
compile.full.timed_out|0|bool(0/1)
runtime.thread.deadline_1khz_miss_rate_pct|0E-16|pct
runtime.thread.deadline_10khz_miss_rate_pct|2.3666666666666667|pct
runtime.thread.deadline_48khz_miss_rate_pct|75.433333333333337|pct
runtime.timer.freq_10khz_p99_ns|105922.00000000000|ns
runtime.timer.freq_10khz_overruns|37.000000000000000|count
runtime.timer.adaptive_auto_p99_ns|23430.000000000000|ns
runtime.ringbuf.contention_1reader_tokens_per_sec|774320661.65807974|items/s
runtime.ringbuf.contention_2readers_tokens_per_sec|937358045.25419307|items/s
runtime.ringbuf.contention_4readers_tokens_per_sec|922007977.92284489|items/s
runtime.ringbuf.contention_8readers_tokens_per_sec|670314476.08530533|items/s
runtime.ringbuf.contention_4readers_read_fail_pct|96.119490773221571|pct
e2e.pipeline_64_samples_per_sec|15317334072.051123|samples/s
e2e.pipeline_256_samples_per_sec|17965782399.540218|samples/s
e2e.pipeline_1024_samples_per_sec|16213741887.600349|samples/s
e2e.socket_64_rx_samples_per_sec|NA|samples/s
e2e.socket_256_rx_samples_per_sec|NA|samples/s
e2e.socket_1024_rx_samples_per_sec|NA|samples/s
e2e.socket_error_count|3|count
<!-- PIPIT_METRICS_END -->
