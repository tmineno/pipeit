# Commit Characterization

- ID: `382c9aed49bd` (source: `staged-diff`)
- HEAD: `c83d7b4`
- Branch: `main`
- Generated: 2026-02-21T21:21:04+09:00
- Previous report: `810ed6bec1f9-characterize.md`

## Commands

- Runtime/E2E: `benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir tmp/performance/382c9aed49bd-20260221-212027/runtime`
- Compile (<=30s): `timeout 30s cargo bench --manifest-path compiler/Cargo.toml --bench compiler_bench -- kpi/full_compile_latency --sample-size 10 --measurement-time 0.10 --warm-up-time 0.05 --output-format bencher`

## Status

| Section | Status | Note |
|---|---|---|
| runtime/e2e | pass | `tmp/performance/382c9aed49bd-20260221-212027/runtime.log` |
| compile | pass | wall=1846.436ms, log=`tmp/performance/382c9aed49bd-20260221-212027/compile.log` |

## Full Compile Latency

| Scenario | ns/iter | Delta vs prev |
|---|---:|---:|
| simple | 9.91K | -848 (-7.88%) |
| multitask | 32.2K | +3.38K (+11.75%) |
| complex | 30.7K | +311 (+1.02%) |
| modal | 36.6K | -2.9K (-7.36%) |

## Runtime Deadline Miss Rate

| Clock | miss_rate_pct | Delta vs prev |
|---|---:|---:|
| 1kHz | 0 | +0 |
| 10kHz | 2.73 | +0.3 (+12.33%) |
| 48kHz | 75.6 | +0.3 (+0.40%) |

## Ring Buffer Contention

| Readers | reader_tokens_per_sec | Delta vs prev |
|---:|---:|---:|
| 1 | 745M | +14.5M (+1.98%) |
| 2 | 748M | -129M (-14.72%) |
| 4 | 1.03G | +66.5M (+6.88%) |
| 8 | 685M | +42.9M (+6.69%) |

## E2E Throughput

| Benchmark | samples_per_sec | Delta vs prev |
|---|---:|---:|
| Pipeline/64 | 15.2G | +24.2M (+0.16%) |
| Pipeline/256 | 17.3G | +143M (+0.83%) |
| Pipeline/1024 | 14.8G | -1.52G (-9.33%) |
| Socket/64 rx | NA | - |
| Socket/256 rx | NA | - |
| Socket/1024 rx | NA | - |

- Socket benchmark errors: `3`
- Socket error message: Failed to bind receiver on localhost:19876

## KPI Snapshot (Stable Keys)

| Key | Value | Unit | Delta vs prev |
|---|---:|---|---:|
| `compile.full.simple_ns_per_iter` | 9.91K | ns/iter | -848 (-7.88%) |
| `compile.full.multitask_ns_per_iter` | 32.2K | ns/iter | +3.38K (+11.75%) |
| `compile.full.complex_ns_per_iter` | 30.7K | ns/iter | +311 (+1.02%) |
| `compile.full.modal_ns_per_iter` | 36.6K | ns/iter | -2.9K (-7.36%) |
| `compile.full.wall_ms` | 1.85K | ms | -12.4 (-0.67%) |
| `compile.full.timed_out` | 0 | bool(0/1) | +0 |
| `runtime.thread.deadline_1khz_miss_rate_pct` | 0 | pct | +0 |
| `runtime.thread.deadline_10khz_miss_rate_pct` | 2.73 | pct | +0.3 (+12.33%) |
| `runtime.thread.deadline_48khz_miss_rate_pct` | 75.6 | pct | +0.3 (+0.40%) |
| `runtime.timer.freq_10khz_p99_ns` | 111K | ns | +1.52K (+1.38%) |
| `runtime.timer.freq_10khz_overruns` | 58 | count | +14 (+31.82%) |
| `runtime.timer.adaptive_auto_p99_ns` | 23K | ns | -2.98K (-11.48%) |
| `runtime.ringbuf.contention_1reader_tokens_per_sec` | 745M | items/s | +14.5M (+1.98%) |
| `runtime.ringbuf.contention_2readers_tokens_per_sec` | 748M | items/s | -129M (-14.72%) |
| `runtime.ringbuf.contention_4readers_tokens_per_sec` | 1.03G | items/s | +66.5M (+6.88%) |
| `runtime.ringbuf.contention_8readers_tokens_per_sec` | 685M | items/s | +42.9M (+6.69%) |
| `runtime.ringbuf.contention_4readers_read_fail_pct` | 96.1 | pct | -0.0754 (-0.08%) |
| `e2e.pipeline_64_samples_per_sec` | 15.2G | samples/s | +24.2M (+0.16%) |
| `e2e.pipeline_256_samples_per_sec` | 17.3G | samples/s | +143M (+0.83%) |
| `e2e.pipeline_1024_samples_per_sec` | 14.8G | samples/s | -1.52G (-9.33%) |
| `e2e.socket_64_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_256_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_1024_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_error_count` | 3 | count | +0 (+0.00%) |

## Artifacts

- Runtime log: `tmp/performance/382c9aed49bd-20260221-212027/runtime.log`
- Compile log: `tmp/performance/382c9aed49bd-20260221-212027/compile.log`
- Runtime JSON dir: `tmp/performance/382c9aed49bd-20260221-212027/runtime`

## Machine Readable Metrics

<!-- PIPIT_METRICS_BEGIN -->
compile.full.simple_ns_per_iter|9909|ns/iter
compile.full.multitask_ns_per_iter|32167|ns/iter
compile.full.complex_ns_per_iter|30746|ns/iter
compile.full.modal_ns_per_iter|36560|ns/iter
compile.full.wall_ms|1846.436|ms
compile.full.timed_out|0|bool(0/1)
runtime.thread.deadline_1khz_miss_rate_pct|0E-16|pct
runtime.thread.deadline_10khz_miss_rate_pct|2.7333333333333334|pct
runtime.thread.deadline_48khz_miss_rate_pct|75.633333333333340|pct
runtime.timer.freq_10khz_p99_ns|111184.00000000000|ns
runtime.timer.freq_10khz_overruns|58.000000000000000|count
runtime.timer.adaptive_auto_p99_ns|22998.000000000000|ns
runtime.ringbuf.contention_1reader_tokens_per_sec|744816887.44274664|items/s
runtime.ringbuf.contention_2readers_tokens_per_sec|747706121.76541471|items/s
runtime.ringbuf.contention_4readers_tokens_per_sec|1031787748.0110786|items/s
runtime.ringbuf.contention_8readers_tokens_per_sec|684945506.79143667|items/s
runtime.ringbuf.contention_4readers_read_fail_pct|96.083673789458402|pct
e2e.pipeline_64_samples_per_sec|15189466359.803217|samples/s
e2e.pipeline_256_samples_per_sec|17302475854.843792|samples/s
e2e.pipeline_1024_samples_per_sec|14778504529.810665|samples/s
e2e.socket_64_rx_samples_per_sec|NA|samples/s
e2e.socket_256_rx_samples_per_sec|NA|samples/s
e2e.socket_1024_rx_samples_per_sec|NA|samples/s
e2e.socket_error_count|3|count
<!-- PIPIT_METRICS_END -->
