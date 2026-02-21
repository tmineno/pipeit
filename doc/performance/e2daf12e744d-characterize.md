# Commit Characterization

- ID: `e2daf12e744d` (source: `staged-diff`)
- HEAD: `e0b8a78`
- Branch: `main`
- Generated: 2026-02-21T21:28:47+09:00
- Previous report: `f66e48a32aec-characterize.md`

## Commands

- Runtime/E2E: `benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir tmp/performance/e2daf12e744d-20260221-212804/runtime`
- Compile (<=30s): `timeout 30s cargo bench --manifest-path compiler/Cargo.toml --bench compiler_bench -- kpi/full_compile_latency --sample-size 10 --measurement-time 0.10 --warm-up-time 0.05 --output-format bencher`

## Status

| Section | Status | Note |
|---|---|---|
| runtime/e2e | pass | `tmp/performance/e2daf12e744d-20260221-212804/runtime.log` |
| compile | pass | wall=1870.560ms, log=`tmp/performance/e2daf12e744d-20260221-212804/compile.log` |

## Full Compile Latency

| Scenario | ns/iter | Delta vs prev |
|---|---:|---:|
| simple | 9.87K | -814 (-7.62%) |
| multitask | 28.7K | +128 (+0.45%) |
| complex | 30.6K | -4.12K (-11.87%) |
| modal | 37.4K | +2K (+5.67%) |

## Runtime Deadline Miss Rate

| Clock | miss_rate_pct | Delta vs prev |
|---|---:|---:|
| 1kHz | 0 | +0 |
| 10kHz | 3.23 | +1.37 (+73.21%) |
| 48kHz | 76.2 | +0.5333 (+0.70%) |

## Ring Buffer Contention

| Readers | reader_tokens_per_sec | Delta vs prev |
|---:|---:|---:|
| 1 | 755M | +36.3M (+5.06%) |
| 2 | 889M | +26.2M (+3.04%) |
| 4 | 1G | +24.9M (+2.54%) |
| 8 | 683M | +48.5M (+7.63%) |

## E2E Throughput

| Benchmark | samples_per_sec | Delta vs prev |
|---|---:|---:|
| Pipeline/64 | 15.1G | -15.8M (-0.10%) |
| Pipeline/256 | 17.7G | -69.7M (-0.39%) |
| Pipeline/1024 | 16G | -233M (-1.44%) |
| Socket/64 rx | 9.31M | +432K (+4.86%) |
| Socket/256 rx | 36.4M | +4.08M (+12.63%) |
| Socket/1024 rx | 45.9M | -2.68M (-5.52%) |

- Socket benchmark errors: `0`

## KPI Snapshot (Stable Keys)

| Key | Value | Unit | Delta vs prev |
|---|---:|---|---:|
| `compile.full.simple_ns_per_iter` | 9.87K | ns/iter | -814 (-7.62%) |
| `compile.full.multitask_ns_per_iter` | 28.7K | ns/iter | +128 (+0.45%) |
| `compile.full.complex_ns_per_iter` | 30.6K | ns/iter | -4.12K (-11.87%) |
| `compile.full.modal_ns_per_iter` | 37.4K | ns/iter | +2K (+5.67%) |
| `compile.full.wall_ms` | 1.87K | ms | +17.8 (+0.96%) |
| `compile.full.timed_out` | 0 | bool(0/1) | +0 |
| `runtime.thread.deadline_1khz_miss_rate_pct` | 0 | pct | +0 |
| `runtime.thread.deadline_10khz_miss_rate_pct` | 3.23 | pct | +1.37 (+73.21%) |
| `runtime.thread.deadline_48khz_miss_rate_pct` | 76.2 | pct | +0.5333 (+0.70%) |
| `runtime.timer.freq_10khz_p99_ns` | 111K | ns | +1.59K (+1.45%) |
| `runtime.timer.freq_10khz_overruns` | 66 | count | +18 (+37.50%) |
| `runtime.timer.adaptive_auto_p99_ns` | 15.5K | ns | -5.44K (-26.03%) |
| `runtime.ringbuf.contention_1reader_tokens_per_sec` | 755M | items/s | +36.3M (+5.06%) |
| `runtime.ringbuf.contention_2readers_tokens_per_sec` | 889M | items/s | +26.2M (+3.04%) |
| `runtime.ringbuf.contention_4readers_tokens_per_sec` | 1G | items/s | +24.9M (+2.54%) |
| `runtime.ringbuf.contention_8readers_tokens_per_sec` | 683M | items/s | +48.5M (+7.63%) |
| `runtime.ringbuf.contention_4readers_read_fail_pct` | 96.2 | pct | +0.0729 (+0.08%) |
| `e2e.pipeline_64_samples_per_sec` | 15.1G | samples/s | -15.8M (-0.10%) |
| `e2e.pipeline_256_samples_per_sec` | 17.7G | samples/s | -69.7M (-0.39%) |
| `e2e.pipeline_1024_samples_per_sec` | 16G | samples/s | -233M (-1.44%) |
| `e2e.socket_64_rx_samples_per_sec` | 9.31M | samples/s | +432K (+4.86%) |
| `e2e.socket_256_rx_samples_per_sec` | 36.4M | samples/s | +4.08M (+12.63%) |
| `e2e.socket_1024_rx_samples_per_sec` | 45.9M | samples/s | -2.68M (-5.52%) |
| `e2e.socket_error_count` | 0 | count | +0 |

## Artifacts

- Runtime log: `tmp/performance/e2daf12e744d-20260221-212804/runtime.log`
- Compile log: `tmp/performance/e2daf12e744d-20260221-212804/compile.log`
- Runtime JSON dir: `tmp/performance/e2daf12e744d-20260221-212804/runtime`

## Machine Readable Metrics

<!-- PIPIT_METRICS_BEGIN -->
compile.full.simple_ns_per_iter|9873|ns/iter
compile.full.multitask_ns_per_iter|28698|ns/iter
compile.full.complex_ns_per_iter|30593|ns/iter
compile.full.modal_ns_per_iter|37375|ns/iter
compile.full.wall_ms|1870.560|ms
compile.full.timed_out|0|bool(0/1)
runtime.thread.deadline_1khz_miss_rate_pct|0E-16|pct
runtime.thread.deadline_10khz_miss_rate_pct|3.2333333333333334|pct
runtime.thread.deadline_48khz_miss_rate_pct|76.233333333333334|pct
runtime.timer.freq_10khz_p99_ns|110998.00000000000|ns
runtime.timer.freq_10khz_overruns|66.000000000000000|count
runtime.timer.adaptive_auto_p99_ns|15467.000000000000|ns
runtime.ringbuf.contention_1reader_tokens_per_sec|754652140.10930514|items/s
runtime.ringbuf.contention_2readers_tokens_per_sec|889069735.70029163|items/s
runtime.ringbuf.contention_4readers_tokens_per_sec|1003848451.1024867|items/s
runtime.ringbuf.contention_8readers_tokens_per_sec|683371244.31832588|items/s
runtime.ringbuf.contention_4readers_read_fail_pct|96.221112536128174|pct
e2e.pipeline_64_samples_per_sec|15065916432.740431|samples/s
e2e.pipeline_256_samples_per_sec|17677806845.393101|samples/s
e2e.pipeline_1024_samples_per_sec|15972281937.715769|samples/s
e2e.socket_64_rx_samples_per_sec|9312629.3368169647|samples/s
e2e.socket_256_rx_samples_per_sec|36380632.348809443|samples/s
e2e.socket_1024_rx_samples_per_sec|45915219.240151942|samples/s
e2e.socket_error_count|0|count
<!-- PIPIT_METRICS_END -->
