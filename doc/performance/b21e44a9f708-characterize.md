# Commit Characterization

- ID: `b21e44a9f708` (source: `staged-diff`)
- HEAD: `3c2085b`
- Branch: `feature/gui-enhance`
- Generated: 2026-02-22T08:18:58+09:00
- Previous report: `b6a0c92149fe-characterize.md`

## Commands

- Runtime/E2E: `benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir tmp/performance/b21e44a9f708-20260222-081815/runtime`
- Compile (<=30s): `timeout 30s cargo bench --manifest-path compiler/Cargo.toml --bench compiler_bench -- kpi/full_compile_latency --sample-size 10 --measurement-time 0.10 --warm-up-time 0.05 --output-format bencher`

## Status

| Section | Status | Note |
|---|---|---|
| runtime/e2e | pass | `tmp/performance/b21e44a9f708-20260222-081815/runtime.log` |
| compile | pass | wall=1913.929ms, log=`tmp/performance/b21e44a9f708-20260222-081815/compile.log` |

## Full Compile Latency

| Scenario | ns/iter | Delta vs prev |
|---|---:|---:|
| simple | 10.5K | -155 (-1.46%) |
| multitask | 31.8K | +1.88K (+6.28%) |
| complex | 31.8K | -3.21K (-9.17%) |
| modal | 36.8K | -136 (-0.37%) |

## Runtime Deadline Miss Rate

| Clock | miss_rate_pct | Delta vs prev |
|---|---:|---:|
| 1kHz | 0 | -0.1 (-100.00%) |
| 10kHz | 5.63 | -0.6 (-9.63%) |
| 48kHz | 76.8 | +0.0667 (+0.09%) |

## Ring Buffer Contention

| Readers | reader_tokens_per_sec | Delta vs prev |
|---:|---:|---:|
| 1 | 659M | +17.3M (+2.70%) |
| 2 | 808M | +69.7M (+9.43%) |
| 4 | 867M | +14.4M (+1.69%) |
| 8 | 634M | -4.59M (-0.72%) |

## E2E Throughput

| Benchmark | samples_per_sec | Delta vs prev |
|---|---:|---:|
| Pipeline/64 | 14.6G | -33.7M (-0.23%) |
| Pipeline/256 | 17.1G | -67M (-0.39%) |
| Pipeline/1024 | 15.5G | -86M (-0.55%) |
| Socket/64 rx | 9M | -212K (-2.30%) |
| Socket/256 rx | 35.5M | -484K (-1.35%) |
| Socket/1024 rx | 49.6M | +3.2M (+6.91%) |

- Socket benchmark errors: `0`

## KPI Snapshot (Stable Keys)

| Key | Value | Unit | Delta vs prev |
|---|---:|---|---:|
| `compile.full.simple_ns_per_iter` | 10.5K | ns/iter | -155 (-1.46%) |
| `compile.full.multitask_ns_per_iter` | 31.8K | ns/iter | +1.88K (+6.28%) |
| `compile.full.complex_ns_per_iter` | 31.8K | ns/iter | -3.21K (-9.17%) |
| `compile.full.modal_ns_per_iter` | 36.8K | ns/iter | -136 (-0.37%) |
| `compile.full.wall_ms` | 1.91K | ms | -24.1K (-92.64%) |
| `compile.full.timed_out` | 0 | bool(0/1) | +0 |
| `runtime.thread.deadline_1khz_miss_rate_pct` | 0 | pct | -0.1 (-100.00%) |
| `runtime.thread.deadline_10khz_miss_rate_pct` | 5.63 | pct | -0.6 (-9.63%) |
| `runtime.thread.deadline_48khz_miss_rate_pct` | 76.8 | pct | +0.0667 (+0.09%) |
| `runtime.timer.freq_10khz_p99_ns` | 31K | ns | +11.4K (+3.82%) |
| `runtime.timer.freq_10khz_overruns` | 135 | count | +21 (+18.42%) |
| `runtime.timer.adaptive_auto_p99_ns` | 192K | ns | -138K (-41.79%) |
| `runtime.ringbuf.contention_1reader_tokens_per_sec` | 659M | items/s | +17.3M (+2.70%) |
| `runtime.ringbuf.contention_2readers_tokens_per_sec` | 808M | items/s | +69.7M (+9.43%) |
| `runtime.ringbuf.contention_4readers_tokens_per_sec` | 867M | items/s | +14.4M (+1.69%) |
| `runtime.ringbuf.contention_8readers_tokens_per_sec` | 634M | items/s | -4.59M (-0.72%) |
| `runtime.ringbuf.contention_4readers_read_fail_pct` | 96.6 | pct | -0.1793 (-0.19%) |
| `e2e.pipeline_64_samples_per_sec` | 14.6G | samples/s | -33.7M (-0.23%) |
| `e2e.pipeline_256_samples_per_sec` | 17.1G | samples/s | -67M (-0.39%) |
| `e2e.pipeline_1024_samples_per_sec` | 15.5G | samples/s | -86M (-0.55%) |
| `e2e.socket_64_rx_samples_per_sec` | 9M | samples/s | -212K (-2.30%) |
| `e2e.socket_256_rx_samples_per_sec` | 35.5M | samples/s | -484K (-1.35%) |
| `e2e.socket_1024_rx_samples_per_sec` | 49.6M | samples/s | +3.2M (+6.91%) |
| `e2e.socket_error_count` | 0 | count | +0 |

## Artifacts

- Runtime log: `tmp/performance/b21e44a9f708-20260222-081815/runtime.log`
- Compile log: `tmp/performance/b21e44a9f708-20260222-081815/compile.log`
- Runtime JSON dir: `tmp/performance/b21e44a9f708-20260222-081815/runtime`

## Machine Readable Metrics

<!-- PIPIT_METRICS_BEGIN -->
compile.full.simple_ns_per_iter|10460|ns/iter
compile.full.multitask_ns_per_iter|31759|ns/iter
compile.full.complex_ns_per_iter|31800|ns/iter
compile.full.modal_ns_per_iter|36772|ns/iter
compile.full.wall_ms|1913.929|ms
compile.full.timed_out|0|bool(0/1)
runtime.thread.deadline_1khz_miss_rate_pct|0E-16|pct
runtime.thread.deadline_10khz_miss_rate_pct|5.6333333333333337|pct
runtime.thread.deadline_48khz_miss_rate_pct|76.833333333333329|pct
runtime.timer.freq_10khz_p99_ns|310149.00000000000|ns
runtime.timer.freq_10khz_overruns|135.00000000000000|count
runtime.timer.adaptive_auto_p99_ns|192214.00000000000|ns
runtime.ringbuf.contention_1reader_tokens_per_sec|659174533.81561279|items/s
runtime.ringbuf.contention_2readers_tokens_per_sec|808171807.01209962|items/s
runtime.ringbuf.contention_4readers_tokens_per_sec|866636844.42900467|items/s
runtime.ringbuf.contention_8readers_tokens_per_sec|633763790.79108393|items/s
runtime.ringbuf.contention_4readers_read_fail_pct|96.604644845850871|pct
e2e.pipeline_64_samples_per_sec|14619500844.006201|samples/s
e2e.pipeline_256_samples_per_sec|17055973196.243315|samples/s
e2e.pipeline_1024_samples_per_sec|15541178441.134573|samples/s
e2e.socket_64_rx_samples_per_sec|8999454.8620532621|samples/s
e2e.socket_256_rx_samples_per_sec|35517167.249179259|samples/s
e2e.socket_1024_rx_samples_per_sec|49556094.532868877|samples/s
e2e.socket_error_count|0|count
<!-- PIPIT_METRICS_END -->
