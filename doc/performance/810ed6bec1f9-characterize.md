# Commit Characterization

- ID: `810ed6bec1f9` (source: `staged-diff`)
- HEAD: `c83d7b4`
- Branch: `main`
- Generated: 2026-02-21T21:20:24+09:00
- Previous report: `prefix-check-characterize.md`

## Commands

- Runtime/E2E: `benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir tmp/performance/810ed6bec1f9-20260221-211947/runtime`
- Compile (<=30s): `timeout 30s cargo bench --manifest-path compiler/Cargo.toml --bench compiler_bench -- kpi/full_compile_latency --sample-size 10 --measurement-time 0.10 --warm-up-time 0.05 --output-format bencher`

## Status

| Section | Status | Note |
|---|---|---|
| runtime/e2e | pass | `tmp/performance/810ed6bec1f9-20260221-211947/runtime.log` |
| compile | pass | wall=1858.799ms, log=`tmp/performance/810ed6bec1f9-20260221-211947/compile.log` |

## Full Compile Latency

| Scenario | ns/iter | Delta vs prev |
|---|---:|---:|
| simple | 10.8K | +821 (+8.26%) |
| multitask | 28.8K | +202 (+0.71%) |
| complex | 30.4K | +609 (+2.04%) |
| modal | 39.5K | +4.27K (+12.12%) |

## Runtime Deadline Miss Rate

| Clock | miss_rate_pct | Delta vs prev |
|---|---:|---:|
| 1kHz | 0 | +0 |
| 10kHz | 2.43 | -0.2333 (-8.75%) |
| 48kHz | 75.3 | -0.3 (-0.40%) |

## Ring Buffer Contention

| Readers | reader_tokens_per_sec | Delta vs prev |
|---:|---:|---:|
| 1 | 73M | -20.3M (-2.70%) |
| 2 | 877M | -43.8M (-4.75%) |
| 4 | 965M | -43.4M (-4.30%) |
| 8 | 642M | -44.3M (-6.46%) |

## E2E Throughput

| Benchmark | samples_per_sec | Delta vs prev |
|---|---:|---:|
| Pipeline/64 | 15.2G | +99.9M (+0.66%) |
| Pipeline/256 | 17.2G | -443M (-2.51%) |
| Pipeline/1024 | 16.3G | +145M (+0.90%) |
| Socket/64 rx | NA | - |
| Socket/256 rx | NA | - |
| Socket/1024 rx | NA | - |

- Socket benchmark errors: `3`
- Socket error message: Failed to bind receiver on localhost:19876

## KPI Snapshot (Stable Keys)

| Key | Value | Unit | Delta vs prev |
|---|---:|---|---:|
| `compile.full.simple_ns_per_iter` | 10.8K | ns/iter | +821 (+8.26%) |
| `compile.full.multitask_ns_per_iter` | 28.8K | ns/iter | +202 (+0.71%) |
| `compile.full.complex_ns_per_iter` | 30.4K | ns/iter | +609 (+2.04%) |
| `compile.full.modal_ns_per_iter` | 39.5K | ns/iter | +4.27K (+12.12%) |
| `compile.full.wall_ms` | 1.86K | ms | +9.32 (+0.50%) |
| `compile.full.timed_out` | 0 | bool(0/1) | +0 |
| `runtime.thread.deadline_1khz_miss_rate_pct` | 0 | pct | +0 |
| `runtime.thread.deadline_10khz_miss_rate_pct` | 2.43 | pct | -0.2333 (-8.75%) |
| `runtime.thread.deadline_48khz_miss_rate_pct` | 75.3 | pct | -0.3 (-0.40%) |
| `runtime.timer.freq_10khz_p99_ns` | 11K | ns | -76 (-0.69%) |
| `runtime.timer.freq_10khz_overruns` | 44 | count | -19 (-30.16%) |
| `runtime.timer.adaptive_auto_p99_ns` | 26K | ns | +6.46K (+33.12%) |
| `runtime.ringbuf.contention_1reader_tokens_per_sec` | 73M | items/s | -20.3M (-2.70%) |
| `runtime.ringbuf.contention_2readers_tokens_per_sec` | 877M | items/s | -43.8M (-4.75%) |
| `runtime.ringbuf.contention_4readers_tokens_per_sec` | 965M | items/s | -43.4M (-4.30%) |
| `runtime.ringbuf.contention_8readers_tokens_per_sec` | 642M | items/s | -44.3M (-6.46%) |
| `runtime.ringbuf.contention_4readers_read_fail_pct` | 96.2 | pct | +0.0501 (+0.05%) |
| `e2e.pipeline_64_samples_per_sec` | 15.2G | samples/s | +99.9M (+0.66%) |
| `e2e.pipeline_256_samples_per_sec` | 17.2G | samples/s | -443M (-2.51%) |
| `e2e.pipeline_1024_samples_per_sec` | 16.3G | samples/s | +145M (+0.90%) |
| `e2e.socket_64_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_256_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_1024_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_error_count` | 3 | count | +0 (+0.00%) |

## Artifacts

- Runtime log: `tmp/performance/810ed6bec1f9-20260221-211947/runtime.log`
- Compile log: `tmp/performance/810ed6bec1f9-20260221-211947/compile.log`
- Runtime JSON dir: `tmp/performance/810ed6bec1f9-20260221-211947/runtime`

## Machine Readable Metrics

<!-- PIPIT_METRICS_BEGIN -->
compile.full.simple_ns_per_iter|10757|ns/iter
compile.full.multitask_ns_per_iter|28785|ns/iter
compile.full.complex_ns_per_iter|30435|ns/iter
compile.full.modal_ns_per_iter|39463|ns/iter
compile.full.wall_ms|1858.799|ms
compile.full.timed_out|0|bool(0/1)
runtime.thread.deadline_1khz_miss_rate_pct|0E-16|pct
runtime.thread.deadline_10khz_miss_rate_pct|2.4333333333333331|pct
runtime.thread.deadline_48khz_miss_rate_pct|75.333333333333329|pct
runtime.timer.freq_10khz_p99_ns|109666.00000000000|ns
runtime.timer.freq_10khz_overruns|44.000000000000000|count
runtime.timer.adaptive_auto_p99_ns|25982.000000000000|ns
runtime.ringbuf.contention_1reader_tokens_per_sec|730325136.16629934|items/s
runtime.ringbuf.contention_2readers_tokens_per_sec|876749969.59107125|items/s
runtime.ringbuf.contention_4readers_tokens_per_sec|965336262.96866047|items/s
runtime.ringbuf.contention_8readers_tokens_per_sec|641997423.56252444|items/s
runtime.ringbuf.contention_4readers_read_fail_pct|96.159102787471639|pct
e2e.pipeline_64_samples_per_sec|15165217704.138485|samples/s
e2e.pipeline_256_samples_per_sec|17159549089.568357|samples/s
e2e.pipeline_1024_samples_per_sec|16300010016.032579|samples/s
e2e.socket_64_rx_samples_per_sec|NA|samples/s
e2e.socket_256_rx_samples_per_sec|NA|samples/s
e2e.socket_1024_rx_samples_per_sec|NA|samples/s
e2e.socket_error_count|3|count
<!-- PIPIT_METRICS_END -->
