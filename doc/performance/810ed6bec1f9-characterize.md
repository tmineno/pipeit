# Commit Characterization

- ID: `810ed6bec1f9` (source: `staged-diff`)
- HEAD: `45e1f2a`
- Branch: `main`
- Generated: 2026-02-21T21:03:54+09:00
- Previous report: none

## Commands

- Runtime/E2E: `/home/tmineno/vibecoding/pipeit/benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir /home/tmineno/vibecoding/pipeit/tmp/performance/810ed6bec1f9-20260221-210317/runtime`
- Compile (<=30s): `timeout 30s cargo bench --manifest-path /home/tmineno/vibecoding/pipeit/compiler/Cargo.toml --bench compiler_bench -- kpi/full_compile_latency --sample-size 10 --measurement-time 0.10 --warm-up-time 0.05 --output-format bencher`

## Status

| Section | Status | Note |
|---|---|---|
| runtime/e2e | pass | `/home/tmineno/vibecoding/pipeit/tmp/performance/810ed6bec1f9-20260221-210317/runtime.log` |
| compile | pass | wall=1878.459ms, log=`/home/tmineno/vibecoding/pipeit/tmp/performance/810ed6bec1f9-20260221-210317/compile.log` |

## Full Compile Latency

| Scenario | ns/iter | Delta vs prev |
|---|---:|---:|
| simple | 10011 | - |
| multitask | 29865 | - |
| complex | 34607 | - |
| modal | 35895 | - |

## Runtime Deadline Miss Rate

| Clock | miss_rate_pct | Delta vs prev |
|---|---:|---:|
| 1kHz | 0E-16 | - |
| 10kHz | 3.0666666666666669 | - |
| 48kHz | 75.533333333333331 | - |

## Ring Buffer Contention

| Readers | reader_tokens_per_sec | Delta vs prev |
|---:|---:|---:|
| 1 | 620003981.34670889 | - |
| 2 | 920751142.63467264 | - |
| 4 | 1038855503.6088904 | - |
| 8 | 670858680.63417172 | - |

## E2E Throughput

| Benchmark | samples_per_sec | Delta vs prev |
|---|---:|---:|
| Pipeline/64 | 14285812481.287878 | - |
| Pipeline/256 | 17671689770.279045 | - |
| Pipeline/1024 | 15937137984.114964 | - |
| Socket/64 rx | NA | - |
| Socket/256 rx | NA | - |
| Socket/1024 rx | NA | - |

- Socket benchmark errors: `3`
- Socket error message: Failed to bind receiver on localhost:19876

## KPI Snapshot (Stable Keys)

| Key | Value | Unit | Delta vs prev |
|---|---:|---|---:|
| `compile.full.simple_ns_per_iter` | 10011 | ns/iter | - |
| `compile.full.multitask_ns_per_iter` | 29865 | ns/iter | - |
| `compile.full.complex_ns_per_iter` | 34607 | ns/iter | - |
| `compile.full.modal_ns_per_iter` | 35895 | ns/iter | - |
| `compile.full.wall_ms` | 1878.459 | ms | - |
| `compile.full.timed_out` | 0 | bool(0/1) | - |
| `runtime.thread.deadline_1khz_miss_rate_pct` | 0E-16 | pct | - |
| `runtime.thread.deadline_10khz_miss_rate_pct` | 3.0666666666666669 | pct | - |
| `runtime.thread.deadline_48khz_miss_rate_pct` | 75.533333333333331 | pct | - |
| `runtime.timer.freq_10khz_p99_ns` | 112496.00000000000 | ns | - |
| `runtime.timer.freq_10khz_overruns` | 54.000000000000000 | count | - |
| `runtime.timer.adaptive_auto_p99_ns` | 25560.000000000000 | ns | - |
| `runtime.ringbuf.contention_1reader_tokens_per_sec` | 620003981.34670889 | items/s | - |
| `runtime.ringbuf.contention_2readers_tokens_per_sec` | 920751142.63467264 | items/s | - |
| `runtime.ringbuf.contention_4readers_tokens_per_sec` | 1038855503.6088904 | items/s | - |
| `runtime.ringbuf.contention_8readers_tokens_per_sec` | 670858680.63417172 | items/s | - |
| `runtime.ringbuf.contention_4readers_read_fail_pct` | 96.121678975995962 | pct | - |
| `e2e.pipeline_64_samples_per_sec` | 14285812481.287878 | samples/s | - |
| `e2e.pipeline_256_samples_per_sec` | 17671689770.279045 | samples/s | - |
| `e2e.pipeline_1024_samples_per_sec` | 15937137984.114964 | samples/s | - |
| `e2e.socket_64_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_256_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_1024_rx_samples_per_sec` | NA | samples/s | - |
| `e2e.socket_error_count` | 3 | count | - |

## Artifacts

- Runtime log: `/home/tmineno/vibecoding/pipeit/tmp/performance/810ed6bec1f9-20260221-210317/runtime.log`
- Compile log: `/home/tmineno/vibecoding/pipeit/tmp/performance/810ed6bec1f9-20260221-210317/compile.log`
- Runtime JSON dir: `/home/tmineno/vibecoding/pipeit/tmp/performance/810ed6bec1f9-20260221-210317/runtime`

## Machine Readable Metrics

<!-- PIPIT_METRICS_BEGIN -->
compile.full.simple_ns_per_iter|10011|ns/iter
compile.full.multitask_ns_per_iter|29865|ns/iter
compile.full.complex_ns_per_iter|34607|ns/iter
compile.full.modal_ns_per_iter|35895|ns/iter
compile.full.wall_ms|1878.459|ms
compile.full.timed_out|0|bool(0/1)
runtime.thread.deadline_1khz_miss_rate_pct|0E-16|pct
runtime.thread.deadline_10khz_miss_rate_pct|3.0666666666666669|pct
runtime.thread.deadline_48khz_miss_rate_pct|75.533333333333331|pct
runtime.timer.freq_10khz_p99_ns|112496.00000000000|ns
runtime.timer.freq_10khz_overruns|54.000000000000000|count
runtime.timer.adaptive_auto_p99_ns|25560.000000000000|ns
runtime.ringbuf.contention_1reader_tokens_per_sec|620003981.34670889|items/s
runtime.ringbuf.contention_2readers_tokens_per_sec|920751142.63467264|items/s
runtime.ringbuf.contention_4readers_tokens_per_sec|1038855503.6088904|items/s
runtime.ringbuf.contention_8readers_tokens_per_sec|670858680.63417172|items/s
runtime.ringbuf.contention_4readers_read_fail_pct|96.121678975995962|pct
e2e.pipeline_64_samples_per_sec|14285812481.287878|samples/s
e2e.pipeline_256_samples_per_sec|17671689770.279045|samples/s
e2e.pipeline_1024_samples_per_sec|15937137984.114964|samples/s
e2e.socket_64_rx_samples_per_sec|NA|samples/s
e2e.socket_256_rx_samples_per_sec|NA|samples/s
e2e.socket_1024_rx_samples_per_sec|NA|samples/s
e2e.socket_error_count|3|count
<!-- PIPIT_METRICS_END -->
