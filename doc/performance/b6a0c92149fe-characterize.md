# Commit Characterization

- ID: `b6a0c92149fe` (source: `staged-diff`)
- HEAD: `3c2085b`
- Branch: `feature/gui-enhance`
- Generated: 2026-02-22T08:17:57+09:00
- Previous report: `382c9aed49bd-characterize.md`

## Commands

- Runtime/E2E: `benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir tmp/performance/b6a0c92149fe-20260222-081650/runtime`
- Compile (<=30s): `timeout 30s cargo bench --manifest-path compiler/Cargo.toml --bench compiler_bench -- kpi/full_compile_latency --sample-size 10 --measurement-time 0.10 --warm-up-time 0.05 --output-format bencher`

## Status

| Section | Status | Note |
|---|---|---|
| runtime/e2e | pass | `tmp/performance/b6a0c92149fe-20260222-081650/runtime.log` |
| compile | pass | wall=25999.386ms, log=`tmp/performance/b6a0c92149fe-20260222-081650/compile.log` |

## Full Compile Latency

| Scenario | ns/iter | Delta vs prev |
|---|---:|---:|
| simple | 10.6K | +706 (+7.12%) |
| multitask | 29.9K | -2.28K (-7.10%) |
| complex | 35K | +4.26K (+13.87%) |
| modal | 36.9K | +348 (+0.95%) |

## Runtime Deadline Miss Rate

| Clock | miss_rate_pct | Delta vs prev |
|---|---:|---:|
| 1kHz | 0.1 | +0.1 |
| 10kHz | 6.23 | +3.5 (+128.05%) |
| 48kHz | 76.8 | +1.13 (+1.50%) |

## Ring Buffer Contention

| Readers | reader_tokens_per_sec | Delta vs prev |
|---:|---:|---:|
| 1 | 642M | -103M (-13.83%) |
| 2 | 739M | -9.2M (-1.23%) |
| 4 | 852M | -18M (-17.40%) |
| 8 | 638M | -46.6M (-6.80%) |

## E2E Throughput

| Benchmark | samples_per_sec | Delta vs prev |
|---|---:|---:|
| Pipeline/64 | 14.7G | -536M (-3.53%) |
| Pipeline/256 | 17.1G | -18M (-1.04%) |
| Pipeline/1024 | 15.6G | +849M (+5.74%) |
| Socket/64 rx | 9.21M | - |
| Socket/256 rx | 36M | - |
| Socket/1024 rx | 46.4M | - |

- Socket benchmark errors: `0`

## KPI Snapshot (Stable Keys)

| Key | Value | Unit | Delta vs prev |
|---|---:|---|---:|
| `compile.full.simple_ns_per_iter` | 10.6K | ns/iter | +706 (+7.12%) |
| `compile.full.multitask_ns_per_iter` | 29.9K | ns/iter | -2.28K (-7.10%) |
| `compile.full.complex_ns_per_iter` | 35K | ns/iter | +4.26K (+13.87%) |
| `compile.full.modal_ns_per_iter` | 36.9K | ns/iter | +348 (+0.95%) |
| `compile.full.wall_ms` | 26K | ms | +24.2K (+1308.08%) |
| `compile.full.timed_out` | 0 | bool(0/1) | +0 |
| `runtime.thread.deadline_1khz_miss_rate_pct` | 0.1 | pct | +0.1 |
| `runtime.thread.deadline_10khz_miss_rate_pct` | 6.23 | pct | +3.5 (+128.05%) |
| `runtime.thread.deadline_48khz_miss_rate_pct` | 76.8 | pct | +1.13 (+1.50%) |
| `runtime.timer.freq_10khz_p99_ns` | 299K | ns | +188K (+168.69%) |
| `runtime.timer.freq_10khz_overruns` | 114 | count | +56 (+96.55%) |
| `runtime.timer.adaptive_auto_p99_ns` | 33K | ns | +307K (+1335.78%) |
| `runtime.ringbuf.contention_1reader_tokens_per_sec` | 642M | items/s | -103M (-13.83%) |
| `runtime.ringbuf.contention_2readers_tokens_per_sec` | 739M | items/s | -9.2M (-1.23%) |
| `runtime.ringbuf.contention_4readers_tokens_per_sec` | 852M | items/s | -18M (-17.40%) |
| `runtime.ringbuf.contention_8readers_tokens_per_sec` | 638M | items/s | -46.6M (-6.80%) |
| `runtime.ringbuf.contention_4readers_read_fail_pct` | 96.8 | pct | +0.7002 (+0.73%) |
| `e2e.pipeline_64_samples_per_sec` | 14.7G | samples/s | -536M (-3.53%) |
| `e2e.pipeline_256_samples_per_sec` | 17.1G | samples/s | -18M (-1.04%) |
| `e2e.pipeline_1024_samples_per_sec` | 15.6G | samples/s | +849M (+5.74%) |
| `e2e.socket_64_rx_samples_per_sec` | 9.21M | samples/s | - |
| `e2e.socket_256_rx_samples_per_sec` | 36M | samples/s | - |
| `e2e.socket_1024_rx_samples_per_sec` | 46.4M | samples/s | - |
| `e2e.socket_error_count` | 0 | count | -3 (-100.00%) |

## Artifacts

- Runtime log: `tmp/performance/b6a0c92149fe-20260222-081650/runtime.log`
- Compile log: `tmp/performance/b6a0c92149fe-20260222-081650/compile.log`
- Runtime JSON dir: `tmp/performance/b6a0c92149fe-20260222-081650/runtime`

## Machine Readable Metrics

<!-- PIPIT_METRICS_BEGIN -->
compile.full.simple_ns_per_iter|10615|ns/iter
compile.full.multitask_ns_per_iter|29883|ns/iter
compile.full.complex_ns_per_iter|35011|ns/iter
compile.full.modal_ns_per_iter|36908|ns/iter
compile.full.wall_ms|25999.386|ms
compile.full.timed_out|0|bool(0/1)
runtime.thread.deadline_1khz_miss_rate_pct|0.10000000000000001|pct
runtime.thread.deadline_10khz_miss_rate_pct|6.2333333333333334|pct
runtime.thread.deadline_48khz_miss_rate_pct|76.766666666666666|pct
runtime.timer.freq_10khz_p99_ns|298743.00000000000|ns
runtime.timer.freq_10khz_overruns|114.00000000000000|count
runtime.timer.adaptive_auto_p99_ns|330201.00000000000|ns
runtime.ringbuf.contention_1reader_tokens_per_sec|641840403.85881877|items/s
runtime.ringbuf.contention_2readers_tokens_per_sec|738509190.62642157|items/s
runtime.ringbuf.contention_4readers_tokens_per_sec|852233367.78452623|items/s
runtime.ringbuf.contention_8readers_tokens_per_sec|638353459.51133192|items/s
runtime.ringbuf.contention_4readers_read_fail_pct|96.783901632840170|pct
e2e.pipeline_64_samples_per_sec|14653196804.608009|samples/s
e2e.pipeline_256_samples_per_sec|17122962485.218195|samples/s
e2e.pipeline_1024_samples_per_sec|15627175358.868486|samples/s
e2e.socket_64_rx_samples_per_sec|9211024.5104762763|samples/s
e2e.socket_256_rx_samples_per_sec|36001464.011877283|samples/s
e2e.socket_1024_rx_samples_per_sec|46352989.024212614|samples/s
e2e.socket_error_count|0|count
<!-- PIPIT_METRICS_END -->
