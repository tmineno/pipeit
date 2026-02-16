# Pipit Performance Analysis Report

Date: 2026-02-17
Scope: post-ADR-014 (adaptive spin defaults + EWMA timer calibration)
Spec reference: `doc/spec/pipit-lang-spec-v0.2.0.md` (`tick_rate`, `timer_spin`, overrun policy, SDF static scheduling)

## 1. Measurement Setup

- Host: AMD Ryzen 9 9950X3D (16C/32T), Linux `6.6.87.2-microsoft-standard-WSL2`

Commands used:

```bash
./benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter pdl --output-dir /tmp/pipit_perf_adaptive
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
| 1kHz | 1000 | 0 | 0.00% | 79.2us | 119.5us |
| 10kHz | 2000 | 50 | 2.50% | 72.0us | 111.1us |
| 48kHz | 2000 | 1513 | 75.65% | 41.3us | 96.2us |
| 100kHz | 2000 | 1754 | 87.70% | 38.0us | 90.3us |
| 1MHz | 1000 | 984 | 98.40% | 36.8us | 71.7us |

Jitter with `timer_spin` at 10kHz:

| timer_spin | Overruns | Avg Latency | P99 Latency |
|---|---:|---:|---:|
| 0ns | 54 | 72.9us | 107.0us |
| 10us (new default) | 15 | 62.1us | 98.6us |
| 50us | 2 | 22.3us | 54.5us |
| **auto (EWMA)** | **0** | **0.6us** | **16.8us** |

Batch vs single (10kHz equivalent):

| Mode | Wall Time | CPU Time | Overruns |
|---|---:|---:|---:|
| K=1 (`BM_Timer_BatchVsSingle/1`) | 2000.1ms | 153.5ms | 454 |
| K=10 (`BM_Timer_BatchVsSingle/10`) | 2000.1ms | 18.4ms | 0 |

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

- Overrun rate jumps from `2.50%` at 10kHz to `75.65%` at 48kHz and `87.70%` at 100kHz.
- Task-level miss rate mirrors this (`75.77%` at 48kHz).

Interpretation:

- The primary bottleneck is timer scheduling granularity/jitter on normal OS scheduling.
- At high rates, `drop` policy protects real-time progression but sacrifices iterations.
- Re-check on the same host reproduced the pattern (`p99` around `107-111us` at 10kHz; overrun around `~75%` at 48kHz and `~88%` at 100kHz), reinforcing that wake-up jitter dominates actor compute at high rate.
- Practical K=1 limit is near where timer `p99` approaches period: at 10kHz period is `100us` while observed `p99` is already `~110us`, so sustained operation above this tends to become overrun-heavy.
- CPU pinning (`taskset`) showed only marginal improvement, suggesting jitter is primarily from sleep/wake scheduling path (plus virtualization overhead in WSL2/Hyper-V), not CPU migration alone.

### B2. Wake-up overhead is amortized only when K-factor is used

Evidence:

- Same effective 10kHz workload: K=1 has `454` overruns, K=10 has `0`.
- CPU time drops from `153.5ms` (K=1) to `18.4ms` (K=10) for equivalent wall-time run.
- At 100k effective Hz, K=100 reduces overruns to `0` and CPU time to `9.37ms`.

Interpretation:

- Current runtime is highly sensitive to wake frequency.
- `tick_rate`/K-factor is the key throughput control, not optional tuning.
- Additional timer re-runs confirm that reducing wake count is the main lever: `K=1` vs `K=10` kept similar wall time but cut CPU time by about `8.3x` and removed overruns in the 10kHz-equivalent case.
- CPU cost per wake is similar order for `K=1` and `K=10`, so most gain comes from amortizing fixed wake-up/scheduler overhead across batched firings, not from making actor compute faster.
- In `BM_Timer_HighFreqBatched`, different effective rates with the same timer wake rate produced nearly identical overrun levels, further indicating wake frequency (`timer_hz`) is the dominant control variable.

### B3. Adaptive EWMA spin eliminates overruns at the cost of CPU

Evidence:

- Adaptive mode at 10kHz: `0` overruns, p99 latency `16.8us`, converged spin window `88.9us`.
- Fixed 10us spin: `15` overruns, p99 `98.6us`.
- Fixed 50us spin: `2` overruns, p99 `54.5us`.
- Adaptive CPU cost: `105.2ms` vs `~16ms` for fixed modes (2000 ticks, 200ms wall time).

Interpretation:

- The EWMA algorithm converges to a spin window (~89us) that closely matches the platform's measured sleep jitter (~70-80us avg), providing near-deadline precision.
- The trade-off is explicit: adaptive uses ~6.5x more CPU than fixed-10us, but achieves zero overruns and sub-20us p99 jitter.
- For latency-critical workloads where zero missed deadlines matter, `set timer_spin = auto` is the recommended option.
- For CPU-constrained workloads, the new default `timer_spin = 10000` (10us) provides a good balance: 72% fewer overruns than no spin, with negligible CPU increase.

### B4. RingBuffer multi-reader path is dominated by retry/backpressure

Evidence:

- Writer throughput degrades `711.3M -> 80.3M` items/s from 1 to 8 readers.
- `read_fail_pct` rises to `98.48%`, indicating heavy spin/retry pressure.

Interpretation:

- The limiting factor is synchronization/retry dynamics under fan-out.
- Single-thread raw copy path is fast (multi-billion items/s), so contention policy is the bottleneck.

### B5. Compiler hot phases are parse + codegen (+ analyze variance)

Evidence:

- Parse and codegen are each ~7-8us in phase measurements.
- Analyze has the widest variance band (`4.39us .. 6.18us`).
- Parse scaling is near-linear and reaches ~40.5us at 40 tasks.

Interpretation:

- Frontend parsing and backend generation are the main compile-time cost centers.
- Analyzer data-structure behavior likely drives variance on non-trivial graphs.

## 5. Tuning Strategy (Prioritized)

### Priority 0: Configuration defaults — DONE (ADR-014)

- [x] Changed `tick_rate` default: `1MHz` → `10kHz`. High-frequency tasks automatically get K-factor batching.
- [x] Changed `timer_spin` default: `0` → `10000` (10us). Immediate jitter reduction with negligible CPU cost.
- [x] Added `set timer_spin = auto` for EWMA-based adaptive spin calibration.
- [x] Added compile-time rate guardrails: warning when effective tick period < 10us.

### Priority 1: Runtime timer path — DONE (ADR-014)

- [x] Adaptive sleep-spin strategy via EWMA jitter calibration (`alpha=1/8`, safety margin `2x`, clamp `[500ns, 100us]`).
- [x] Rate guardrails in scheduler: warning when requested clock implies chronic overrun.
- [ ] Track long-run drift KPI in nightly benches (not per-PR).

### Priority 2: RingBuffer contention path

- Introduce bounded backoff/yield in failure loops to reduce retry storm.
- Optimize multi-reader progress accounting for fan-out reads.
- Add chunk-size aware guidance in docs (`64` or `256` token chunks when possible).

### Priority 3: Compiler latency path

- Cache actor registry/header-derived metadata across benchmark iterations.
- Reduce allocation churn in parser/analyzer (arena/interning strategy).
- Add phase-specific memory counters to correlate analyze variance with graph size.

## 6. KPI Targets for Next Iteration

| KPI | Baseline | Post-ADR-014 | Next Target |
|---|---:|---:|---:|
| Timer overruns @10kHz (spin=0) | 54 | — | — |
| Timer overruns @10kHz (spin=10us, default) | — | 15 | <=10 |
| Timer overruns @10kHz (adaptive) | — | **0** | keep 0 |
| Timer p99 @10kHz (spin=0) | 107.0us | — | — |
| Timer p99 @10kHz (spin=10us, default) | — | 98.6us | <=90us |
| Timer p99 @10kHz (adaptive) | — | **16.8us** | <=20us |
| Batch overruns @10kHz K=10 | 0 | 0 | keep 0 |
| RingBuffer writer throughput @8 readers | 80.3M/s | 80.3M/s | >=120M/s |
| RingBuffer read fail @8 readers | 98.48% | 98.48% | <=96% |
| Compiler full compile (complex) | 22.04-22.52us | 22.04-22.52us | <=20us |
| Parse scaling @40 tasks | 40.39-40.54us | 40.39-40.54us | <=36us |

## 7. Conclusion

ADR-014 changes address the two highest-priority tuning targets from the initial analysis:

1. **Default tick_rate lowered to 10kHz** — high-frequency tasks (>10kHz) now automatically batch via K-factor, eliminating the most common overrun scenario without user intervention.
2. **Default timer_spin raised to 10us** — reduces overruns by 72% (54 → 15 at 10kHz) with negligible CPU impact.
3. **Adaptive EWMA spin (`timer_spin = auto`)** — achieves zero overruns and p99 latency of 16.8us by self-calibrating to platform jitter. Trade-off is higher CPU usage (~6.5x), appropriate for latency-critical workloads.
4. **Compile-time rate guardrails** — warn when effective timer rate exceeds OS scheduler capability (~100kHz).

Remaining bottlenecks are ring-buffer contention (Priority 2) and compiler hot-path latency (Priority 3), which are unchanged by this iteration.
