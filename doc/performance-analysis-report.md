# Pipit Performance Analysis Report

Date: 2026-02-16  
Scope: current implementation baseline after benchmark suite simplification  
Spec reference: `doc/spec/pipit-lang-spec-v0.2.0.md` (`tick_rate`, `timer_spin`, overrun policy, SDF static scheduling)

## 1. Measurement Setup

- Host: AMD Ryzen 9 9950X3D (16C/32T), Linux `6.6.87.2-microsoft-standard-WSL2`
Commands used:

```bash
./benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter pdl --output-dir /tmp/pipit_perf_refresh
cargo bench --manifest-path compiler/Cargo.toml --bench compiler_bench -- "kpi/" --sample-size 10 --measurement-time 0.5 --warm-up-time 0.1
```

## 2. Spec-Aligned KPI Definition

The following KPIs are aligned to the language/runtime model in `v0.2.0`:

- `KPI-R1` Runtime deadline quality: task `miss_rate_pct` by clock rate (`set overrun = drop` behavior).
- `KPI-R2` Timer precision: jitter (`p99_ns`) and overrun counts with and without `set timer_spin`.
- `KPI-R3` K-factor effectiveness: impact of `set tick_rate` and `K = ceil(clock_freq / tick_rate)` on overruns and CPU cost.
- `KPI-R4` Shared buffer scalability: writer throughput plus `read_fail_pct`/`write_fail_pct` under multi-reader contention.
- `KPI-C1` Compiler latency: parse latency and full compile latency on representative programs.
- `KPI-C2` Compiler phase bottleneck: per-phase latency (`parse/resolve/graph/analyze/schedule/codegen`).
- `KPI-C3` Compiler scaling: parse latency growth vs number of tasks.
- `KPI-E2E1` End-to-end PDL health: runtime stats (`ticks`, `missed`, `avg_latency`, `max_latency`).

## 3. Current Results

### 3.1 Timer and Thread Runtime

Timer frequency sweep:

| Frequency | Ticks | Overruns | Overrun Rate | Avg Latency | P99 Latency |
|---|---:|---:|---:|---:|---:|
| 1kHz | 1000 | 0 | 0.00% | 79.3us | 112.0us |
| 10kHz | 2000 | 57 | 2.85% | 72.7us | 111.4us |
| 48kHz | 2000 | 1511 | 75.55% | 42.5us | 93.9us |
| 100kHz | 2000 | 1753 | 87.65% | 39.1us | 94.9us |
| 1MHz | 1000 | 983 | 98.30% | 36.8us | 71.8us |

Jitter with `timer_spin` at 10kHz:

| timer_spin | Overruns | Avg Latency | P99 Latency |
|---|---:|---:|---:|
| 0ns | 52 | 74.1us | 113.9us |
| 10us | 18 | 63.0us | 99.1us |
| 50us | 3 | 24.1us | 61.7us |

Batch vs single (10kHz equivalent):

| Mode | Wall Time | CPU Time | Overruns |
|---|---:|---:|---:|
| K=1 (`BM_Timer_BatchVsSingle/1`) | 2000.07ms | 162.99ms | 542 |
| K=10 (`BM_Timer_BatchVsSingle/10`) | 2000.08ms | 18.86ms | 0 |

Task deadline miss rate:

| Clock | Miss Rate | Missed |
|---|---:|---:|
| 1kHz | 0.00% | 0 |
| 10kHz | 2.97% | 89 |
| 48kHz | 75.77% | 2273 |

K-factor batching (`effective_hz = 100kHz`):

| K | Timer Hz | Overruns | CPU Time |
|---:|---:|---:|---:|
| 1 | 100kHz | 87913 | 105.39ms |
| 10 | 10kHz | 288 | 84.24ms |
| 100 | 1kHz | 0 | 9.37ms |

### 3.2 Shared Buffer (Ring Buffer)

Contention results:

| Readers | Writer Throughput (items/s) | Read Fail % | Write Fail % |
|---:|---:|---:|---:|
| 1 | 711.3M | 75.62% | 63.22% |
| 2 | 432.3M | 90.70% | 55.62% |
| 4 | 229.3M | 96.44% | 34.34% |
| 8 | 80.3M | 98.48% | 27.90% |

Single-thread scaling:

| Benchmark | Throughput (items/s) |
|---|---:|
| Throughput baseline | 1.44M |
| Size scaling (256 .. 16K) | 5.71B .. 6.41B |
| Chunk scaling 16 | 5.54B |
| Chunk scaling 64 | 12.14B |
| Chunk scaling 256 | 16.54B |

### 3.3 Compiler

Parse latency (`kpi/parse_latency`):

| Scenario | Latency (range) |
|---|---:|
| simple | 3.67us .. 3.70us |
| multitask | 6.00us .. 6.16us |
| complex | 7.07us .. 7.18us |
| modal | 7.42us .. 7.61us |

Full compile latency (`kpi/full_compile_latency`):

| Scenario | Latency (range) |
|---|---:|
| simple | 8.00us .. 8.67us |
| multitask | 20.51us .. 20.84us |
| complex | 22.04us .. 22.52us |
| modal | 24.86us .. 25.21us |

Phase latency for complex pipeline (`kpi/phase_latency/complex`):

| Phase | Latency (range) |
|---|---:|
| parse | 7.10us .. 7.19us |
| resolve | 1.43us .. 1.57us |
| graph | 2.50us .. 3.00us |
| analyze | 4.39us .. 6.18us |
| schedule | 2.93us .. 3.26us |
| codegen | 7.83us .. 8.17us |

Parse scaling (`kpi/parse_scaling`):

| Tasks | Parse Latency (range) |
|---:|---:|
| 1 | 3.72us .. 3.75us |
| 5 | 7.60us .. 7.94us |
| 10 | 12.11us .. 12.23us |
| 20 | 21.18us .. 21.29us |
| 40 | 40.39us .. 40.54us |

### 3.4 End-to-End PDL

`pdl_bench` summary:

- `simple`: `ticks=9658`, `missed=343`, `avg_latency=75977ns`, `max_latency=227394ns`
- `modal/adaptive`: `ticks=257`, `missed=7`, `avg_latency=82575ns`, `max_latency=193475ns`
- `multitask/producer`: `ticks=3`, `missed=0`, `avg_latency=80537ns`
- `multitask/consumer`: `ticks=1`, `missed=0`, `avg_latency=79852ns`
- `sdr/capture`: `ticks=3`, `missed=1`, `avg_latency=76380ns`

## 4. Bottleneck Analysis

### B1. OS timer wake-up limits dominate above ~10kHz

Evidence:

- Overrun rate jumps from `2.85%` at 10kHz to `75.55%` at 48kHz and `87.65%` at 100kHz.
- Task-level miss rate mirrors this (`75.77%` at 48kHz).

Interpretation:

- The primary bottleneck is timer scheduling granularity/jitter on normal OS scheduling.
- At high rates, `drop` policy protects real-time progression but sacrifices iterations.

### B2. Wake-up overhead is amortized only when K-factor is used

Evidence:

- Same effective 10kHz workload: K=1 has `542` overruns, K=10 has `0`.
- CPU time drops from `162.99ms` (K=1) to `18.86ms` (K=10) for equivalent wall-time run.
- At 100k effective Hz, K=100 reduces overruns to `0` and CPU time to `9.37ms`.

Interpretation:

- Current runtime is highly sensitive to wake frequency.
- `tick_rate`/K-factor is the key throughput control, not optional tuning.

### B3. RingBuffer multi-reader path is dominated by retry/backpressure

Evidence:

- Writer throughput degrades `711.3M -> 80.3M` items/s from 1 to 8 readers.
- `read_fail_pct` rises to `98.48%`, indicating heavy spin/retry pressure.

Interpretation:

- The limiting factor is synchronization/retry dynamics under fan-out.
- Single-thread raw copy path is fast (multi-billion items/s), so contention policy is the bottleneck.

### B4. Compiler hot phases are parse + codegen (+ analyze variance)

Evidence:

- Parse and codegen are each ~7-8us in phase measurements.
- Analyze has the widest variance band (`4.39us .. 6.18us`).
- Parse scaling is near-linear and reaches ~40.5us at 40 tasks.

Interpretation:

- Frontend parsing and backend generation are the main compile-time cost centers.
- Analyzer data-structure behavior likely drives variance on non-trivial graphs.

## 5. Tuning Strategy (Prioritized)

### Priority 0: Configuration defaults (no architecture change)

- Define workload profiles and default `tick_rate`:
  - low-latency profile: high `tick_rate`, low K
  - throughput profile: lower `tick_rate`, higher K
- Recommend `timer_spin=10us` as baseline low-risk jitter improvement.
- Reserve `timer_spin=50us` for strict-latency deployments with CPU budget.

### Priority 1: Runtime timer path

- Add adaptive sleep-spin strategy:
  - coarse sleep until `deadline - spin_window`
  - short calibrated spin until deadline
- Add rate guardrails in generated runtime config:
  - warning when requested clock + host behavior implies chronic overrun.
- Track long-run drift KPI in nightly benches (not per-PR).

### Priority 2: RingBuffer contention path

- Introduce bounded backoff/yield in failure loops to reduce retry storm.
- Optimize multi-reader progress accounting for fan-out reads.
- Add chunk-size aware guidance in docs (`64` or `256` token chunks when possible).

### Priority 3: Compiler latency path

- Cache actor registry/header-derived metadata across benchmark iterations.
- Reduce allocation churn in parser/analyzer (arena/interning strategy).
- Add phase-specific memory counters to correlate analyze variance with graph size.

## 6. KPI Targets for Next Iteration

| KPI | Current | Next Target |
|---|---:|---:|
| Runtime miss rate @10kHz | 2.97% | <=1.0% |
| Runtime miss rate @48kHz | 75.77% | <=30% |
| Timer p99 @10kHz (`spin=0`) | 113.9us | <=100us |
| Timer overruns @10kHz equivalent (K=10) | 0 | keep 0 |
| RingBuffer writer throughput @8 readers | 80.3M/s | >=120M/s |
| RingBuffer read fail @8 readers | 98.48% | <=96% |
| Compiler full compile (complex) | 22.04-22.52us | <=20us |
| Parse scaling @40 tasks | 40.39-40.54us | <=36us |

## 7. Conclusion

Current implementation is healthy for low-rate to moderate-rate runtime workloads, but high-frequency operation is bounded by OS wake-up behavior and multi-reader contention policy. The highest-impact tuning path is:

1. Make K-factor/tick-rate profile-driven by default.
2. Improve timer wake precision with adaptive spin.
3. Reduce ring-buffer retry pressure under contention.
4. Trim parser/codegen/analyze overhead in compiler hot path.
