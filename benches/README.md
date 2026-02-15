# Pipit Benchmarks

Comprehensive benchmark suite for performance characterization (v0.2.1).

## Quick Start

```bash
# Run all benchmarks
./run_all.sh

# Run specific category
./run_all.sh --filter actor
./run_all.sh --filter ringbuf --filter timer

# Custom output directory
./run_all.sh --output-dir /tmp/my_results

# Generate a human-readable report from generated JSON files
./run_all.sh --filter actor --filter thread --report
./run_all.sh --report --report-bench actor_bench --report-bench thread_bench

# Validate canonical JSON outputs
./run_all.sh --filter runtime --validate

# Compare current canonical outputs against a baseline directory
./run_all.sh --filter runtime --validate \
  --compare-baseline-dir ./baselines/nightly \
  --compare-threshold-pct 5
```

Categories: `compiler`, `runtime`, `ringbuf`, `timer`, `thread`, `actor`, `pdl`, `affinity`, `memory`, `latency`, `perf`, `all`

## Requirements

- Rust toolchain (for compiler benchmarks)
- C++20 compiler (g++ or clang++)
- Google Benchmark library (for C++ benchmarks)
- `jq` (for report generation, canonical validation, and baseline comparison)

```bash
# Ubuntu/Debian
sudo apt install libbenchmark-dev jq
```

## Benchmark Suites

### 1. Compiler Benchmarks (Criterion)

**File**: `../compiler/benches/compiler_bench.rs`

Measures Pipit compiler performance across pipeline complexity levels:

| Group | What it measures |
|-------|-----------------|
| `parse` | Parser performance (simple/medium/complex/modal) |
| `parse_stress` | Stress tests (100+ actors, 5-level nesting, 50 fan-out, 10 modes) |
| `parse_scaling` | Parse time vs number of tasks (1/5/10/20/50) |
| `full_pipeline` | Parse + resolve + graph (empty registry) |
| `full_pipeline_loaded` | Full pipeline with loaded actor registry (parse through codegen) |
| `phase/*` | Per-phase cost (parse, resolve, graph, analyze, schedule, codegen) |
| `codegen_size` | Code size proxy |

```bash
cargo bench --manifest-path ../compiler/Cargo.toml
```

### 2. Runtime Primitive Benchmarks

**File**: `runtime_bench.cpp`

Low-level performance of `libpipit` primitives:

- `BM_RingBuffer_Write/Read/RoundTrip/MultiReader`
- `BM_Timer_Tick_1kHz/10kHz`
- `BM_TaskStats_Record`
- `BM_AtomicParam_Load/Store`

```bash
c++ -std=c++20 -O3 -march=native -I ../runtime/libpipit/include \
    runtime_bench.cpp -lbenchmark -lpthread -o /tmp/runtime_bench
/tmp/runtime_bench
```

### 3. Ring Buffer Stress Tests

**File**: `ringbuf_bench.cpp`

Multi-threaded ring buffer performance:

- `BM_RingBuffer_Throughput` - Sustained write+read (1M tokens, writer+reader threads)
- `BM_RingBuffer_Contention/{2,4,8,16}readers` - Multi-reader contention scaling
- `BM_RingBuffer_SizeScaling/{64,256,1K,4K,16K,64K}` - Buffer capacity effects
- `BM_RingBuffer_ChunkScaling/{1,4,16,64,256,1024}` - Transfer size effects

```bash
c++ -std=c++20 -O3 -march=native -I ../runtime/libpipit/include \
    ringbuf_bench.cpp -lbenchmark -lpthread -o /tmp/ringbuf_bench
/tmp/ringbuf_bench
```

### 4. Timer Precision Benchmarks

**File**: `timer_bench.cpp`

Timer accuracy and jitter characterization (Google Benchmark + manual timing):

- **Frequency sweep**: 1Hz to 1MHz in decade steps
- **Jitter histogram**: 10,000 ticks at 10kHz with percentile breakdown
- **Overrun recovery**: Force overrun, measure reset_phase() recovery
- **Wake-up latency**: Best/worst/median at 1kHz

```bash
c++ -std=c++20 -O3 -march=native -I ../runtime/libpipit/include \
    timer_bench.cpp -lbenchmark -lpthread -o /tmp/timer_bench
/tmp/timer_bench --benchmark_format=json --benchmark_out=/tmp/timer_bench.json
```

### 5. Thread Scheduling Benchmarks

**File**: `thread_bench.cpp`

Task scheduling and threading overhead:

- `BM_ThreadCreateJoin` - Thread create + join cost
- `BM_ContextSwitch` - Atomic ping-pong round-trip
- `BM_EmptyPipeline` - Minimal timer+actor loop (framework overhead)
- `BM_TaskScaling/{1,2,4,8,16,32}` - Concurrent task scaling
- `BM_TimerOverhead` - Pure timer object overhead

```bash
c++ -std=c++20 -O3 -march=native -I ../runtime/libpipit/include \
    thread_bench.cpp -lbenchmark -lpthread -o /tmp/thread_bench
/tmp/thread_bench
```

### 6. Actor Microbenchmarks

**File**: `actor_bench.cpp`

Per-actor firing cost (isolated from timer/buffer overhead):

- Arithmetic: `mul`, `add`, `sub`, `div`, `abs`, `sqrt`
- FFT: N=64, 256, 1024, 4096
- FIR: 5-tap, 16-tap, 64-tap
- Statistics: `mean`, `rms`, `min`, `max` (N=64)
- Transform: `c2r`, `mag` (N=256), `decimate` (N=10)

```bash
c++ -std=c++20 -O3 -march=native -I ../runtime/libpipit/include \
    actor_bench.cpp -lbenchmark -lpthread -o /tmp/actor_bench
/tmp/actor_bench
```

### 7. End-to-End PDL Benchmarks

**Files**: `pdl_bench.sh`, `pdl/*.pdl`

Full pipeline performance from compiled PDL programs:

| PDL Program | Description | Rate |
|------------|-------------|------|
| `simple.pdl` | Single task baseline | 100 kHz |
| `multitask.pdl` | Shared buffer communication | 10 kHz / 5 kHz |
| `modal.pdl` | CSDF mode switching | 50 kHz |
| `complex.pdl` | Taps + FIR + decimation | 100 kHz + 10 kHz |
| `sdr_receiver.pdl` | FFT + FIR + demod chain | 1 MHz + 100 kHz |
| `audio_chain.pdl` | Audio effects pipeline | 48 kHz |
| `sensor_fusion.pdl` | 5 sensor channels + aggregator | 1 kHz |

```bash
./pdl_bench.sh
```

### 8. CPU Affinity Benchmarks

**File**: `affinity_bench.cpp`

Measures how thread-to-core pinning affects ring buffer throughput. Probes CPU
topology from sysfs at startup (SMT siblings, physical cores, CCD boundaries).

- `BM_Affinity_Unpinned` - Baseline (OS scheduler decides)
- `BM_Affinity_SameCore` - Writer/reader on probed SMT siblings
- `BM_Affinity_AdjacentCore` - Writer/reader on adjacent physical cores
- `BM_Affinity_DistantCore` - Writer/reader on most-distant cores
- `BM_Affinity_TaskScaling/{1..32}` - N threads pinned to distinct cores

### 9. Memory Subsystem Benchmarks

**File**: `memory_bench.cpp`

Memory characteristics of Pipit runtime components:

- `BM_Memory_Footprint` - sizeof() for RingBuffer, Timer, TaskStats (via counters)
- `BM_Memory_CacheLineUtil/{1..1024}` - Cache line efficiency at different chunk sizes
- `BM_Memory_FalseSharing/{1..8}readers` - False sharing detection
- `BM_Memory_Bandwidth/{4..16384}` - Memory bandwidth saturation (KB)
- `BM_Memory_PageFault_Cold/Warm` - Page fault impact

### 10. Latency Breakdown Benchmarks

**File**: `latency_bench.cpp`

Detailed latency analysis with percentile tracking (Google Benchmark + counters):

- Per-actor firing: mul, add, fft, fir, mean, c2r, rms (min/avg/p90/p99/p999/max)
- Timer overhead vs actual work ratio
- Ring buffer read/write vs compute time budget
- Task wake-up to first instruction latency
- End-to-end pipeline latency: mul -> fir -> mean

```bash
c++ -std=c++20 -O3 -march=native -I ../runtime/libpipit/include -I ../examples \
    latency_bench.cpp -lbenchmark -lpthread -o /tmp/latency_bench
/tmp/latency_bench --benchmark_format=json --benchmark_out=/tmp/latency_bench.json
```

### 11. Perf-Based Analysis

**Directory**: `perf/`

Shell scripts wrapping existing benchmarks with `perf stat`/`perf record`. All
scripts probe the environment at startup (CPU topology, cache sizes, available
perf events) and adapt dynamically.

| Script | What it measures |
|--------|-----------------|
| `perf_ringbuf.sh` | Ring buffer L1/L2/L3 cache hit rates, TLB behavior |
| `perf_numa.sh` | NUMA/CCD topology effects (graceful degradation on single-node) |
| `perf_affinity.sh` | Affinity + cache/context-switch/migration correlation |
| `perf_actor.sh` | Vectorization (IPC), pipeline stalls, data dependency analysis |
| `perf_memory.sh` | Page faults, cache line utilization, false sharing via perf |
| `perf_profile.sh` | CPU hotspots, branch mispredictions, cache/TLB miss rates |
| `perf_flamegraph.sh` | Flame graph SVGs (auto-downloads FlameGraph tools) |
| `perf_contention.sh` | Atomic contention scaling, memory ordering overhead |

Requires: `perf` (Linux). Optional: `numactl`, `taskset`.

```bash
# Run individual script
bash perf/perf_actor.sh

# Run all perf analysis
./run_all.sh --filter perf
```

## Output Format

- **Canonical JSON (Phase 1)**: One file per category in `results/`
  - `compiler.canonical.json`
  - `runtime.canonical.json`
  - `ringbuf.canonical.json`
  - `timer.canonical.json`
  - `thread.canonical.json`
  - `actor.canonical.json`
  - `pdl.canonical.json`
  - `affinity.canonical.json`
  - `memory.canonical.json`
  - `latency.canonical.json`
  - `perf.canonical.json`
  - `pdl` naming: `pdl/<program>/task:<task>` and `pdl/<program>/buffer:<buffer>`
  - `perf` naming: `perf/<scenario>/<variant>`
- **Google Benchmark**: JSON files in `results/` (e.g., `actor_bench.json`)
- **Criterion**: HTML reports in `../target/criterion/`
- **PDL bench**: Text output in `results/pdl_bench.txt`
- **Perf analysis**: Text/JSON in `results/perf_*.txt` and `results/perf_*.json`
- **Flame graphs**: SVG in `results/flamegraph_*.svg` (open in browser)
- **Human-readable summary report**: Markdown in `results/benchmark_report.md`
- **Baseline comparison report**: Markdown in `results/baseline_comparison.md`

### Human-Readable Report

Convert selected benchmark JSON files into a Markdown report:

```bash
# Auto-generate report after running benchmarks
./run_all.sh --report

# Limit report to selected bench JSON files
./run_all.sh --report --report-bench actor_bench --report-bench thread_bench

# Generate report from existing JSON files only
./json_report.sh --input-dir ./results --bench actor_bench --bench thread_bench
```

### Canonical Validation Utility

Validate canonical JSON artifacts (`*.canonical.json`) with naming/shape checks:

```bash
# Validate all canonical JSON files in results/
./validate_canonical_results.sh --input-dir ./results

# Validate selected canonical JSON files
./validate_canonical_results.sh \
  --file ./results/runtime.canonical.json \
  --file ./results/actor.canonical.json
```

### Baseline Comparison Utility

Compare current canonical results against a baseline directory:

```bash
./compare_canonical_results.sh \
  --baseline-dir ./baselines/nightly \
  --current-dir ./results \
  --threshold-pct 5 \
  --output ./results/baseline_comparison.md
```

`run_all.sh` can invoke both utilities in one run:

```bash
./run_all.sh \
  --filter runtime \
  --output-dir ./results \
  --validate \
  --compare-baseline-dir ./baselines/nightly \
  --compare-allow-missing-baseline
```

### CI Integration

- `CI` workflow (`.github/workflows/ci.yml`) runs a benchmark smoke lane
  (`runtime`) with canonical validation.
- `Benchmark Nightly` workflow (`.github/workflows/bench-nightly.yml`) runs the
  `perf` lane with canonical validation and baseline comparison.

### Canonicalization Utility

You can also convert individual outputs manually:

```bash
# Example: gbench JSON -> canonical JSON
./canonicalize_results.sh --kind gbench --suite timer \
  --input ./results/timer_bench.json \
  --output ./results/timer.canonical.json
```

## Expected Performance

On modern hardware (x86_64, 3+ GHz):

- RingBuffer write/read: ~10-20 ns per token
- Timer tick (1 kHz): ~1-5 us overhead
- TaskStats record: ~5-10 ns
- Atomic param load/store: ~1-2 ns
- Actor mul (N=64): ~10-30 ns per firing
- Actor FFT (N=256): ~5-15 us per firing
- Simple pipeline (100 kHz): ~70-80 us avg latency
- Thread wake-up: ~40-70 us median
