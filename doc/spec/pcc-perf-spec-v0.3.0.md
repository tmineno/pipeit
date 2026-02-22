# Feature: pcc Performance KPI and Benchmark Test Spec

Version: 0.3.0

## 1. Goal

Define the stable performance KPIs and benchmark test specification for `pcc` and related runtime/E2E benchmark suites.

## 2. Non-goals

- This document does not contain run-by-run analysis narratives.
- This document does not replace benchmark raw artifacts under `doc/performance/`.
- This document does not define implementation details for optimization work.

## 3. KPI Definitions

- `KPI-R1` Runtime deadline quality: task `miss_rate_pct` by clock rate (`set overrun = drop` behavior).
- `KPI-R2` Timer precision: jitter (`p99_ns`) and overrun counts with and without `set timer_spin`.
- `KPI-R3` K-factor effectiveness: impact of `set tick_rate` and `K = ceil(clock_freq / tick_rate)` on overruns and CPU cost.
- `KPI-R4` Shared buffer scalability: writer throughput plus `read_fail_pct`/`write_fail_pct` under multi-reader contention.
- `KPI-C1` Compiler latency: parse latency and full compile latency on representative programs.
- `KPI-C2` Compiler phase bottleneck: per-phase latency (`parse/resolve/graph/analyze/schedule/codegen`).
- `KPI-C3` Compiler scaling: parse latency growth vs number of tasks.
- `KPI-E2E1` End-to-end PDL health: runtime stats (`ticks`, `missed`, `avg_latency`, `max_latency`).
- `KPI-E2E2` Pipeline max throughput: CPU-bound ceiling (samples/s) for `constant -> mul -> mul` chain, no timer.
- `KPI-E2E3` Socket loopback max throughput: sustained receiver throughput (samples/s) through PPKT/UDP localhost.

## 4. KPI Target Table (Current Baseline and Next Target)

| KPI | Baseline | Post-ADR-014 | Current (2026-02-21) | Next Target |
|---|---:|---:|---:|---:|
| Timer overruns @10kHz (spin=0) | 54 | - | 45 | - |
| Timer overruns @10kHz (spin=10us, default) | - | 15 | 18 | <=10 |
| Timer overruns @10kHz (adaptive) | - | **0** | **0** | keep 0 |
| Timer p99 @10kHz (spin=0) | 107.0us | - | 109.6us | - |
| Timer p99 @10kHz (spin=10us, default) | - | 98.6us | 97.6us | <=90us |
| Timer p99 @10kHz (adaptive) | - | **16.8us** | **20.5us** | <=20us |
| Batch overruns @10kHz K=10 | 0 | 0 | 0 | keep 0 |
| RingBuffer writer throughput @8 readers | 80.3M/s | 80.3M/s | 82.0M/s | >=120M/s |
| RingBuffer read fail @8 readers | 98.48% | 98.48% | 98.40% | <=96% |
| Compiler full compile (complex) | 22.04-22.52us | 22.04-22.52us | 30.26-30.86us | <=28us |
| Compiler analyze (complex) | 4.39-6.18us | 4.39-6.18us | 7.90-7.95us | <=7us |
| Compiler codegen (complex) | 7.83-8.17us | 7.83-8.17us | 11.21-11.33us | <=10us |
| Parse scaling @40 tasks | 40.39-40.54us | 40.39-40.54us | 42.61-43.37us | <=40us |
| Pipeline throughput N=64 (no timer) | - | 18.8G/s | 18.2G/s | >=18G/s |
| Pipeline throughput N=1024 (no timer) | - | 16.1G/s | 15.1G/s | >=15G/s |
| Socket loopback RX N=64 | - | 9.2M/s | 9.3M/s | >=9M/s |
| Socket loopback RX N=1024 | - | 50.5M/s | 47.5M/s | >=45M/s |

Notes:

- Compiler KPI levels changed between Post-ADR-014 and Current due to v0.3.2 and v0.3.4 feature scope increase (not treated as a direct regression).
- Runtime and E2E KPI baselines were refreshed on 2026-02-21.

## 5. Test Spec

### 5.1 Required benchmark suites

- Runtime benches: ring buffer, timer, thread.
- Compiler benches: `kpi/parse_latency`, `kpi/full_compile_latency`, `kpi/phase_latency`, `kpi/parse_scaling`.
- End-to-end benches: `pdl_bench`, `BM_E2E_PipelineOnly`, `BM_E2E_SocketLoopback`.

### 5.2 Standard commands

```bash
# Runtime + E2E
./benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir tmp/bench_<timestamp>

# Compiler stable A/B
./benches/compiler_bench_stable.sh --baseline-ref <git-ref> --sample-size 30 --measurement-time 0.8 --warm-up-time 0.2
```

### 5.3 Acceptance criteria

- Commands complete successfully.
- KPI keys are present in outputs/artifacts for all required suites.
- Next Target thresholds in section 4 are used as the benchmark evaluation contract.
- Any threshold miss must be reported with metric name, observed value, and command context.

## 6. Failure Modes

- Missing benchmark artifacts or missing KPI keys.
- Non-reproducible output due to command or environment mismatch.
- KPI threshold misses against section 4 targets.
