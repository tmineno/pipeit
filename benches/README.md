# Pipit Benchmarks

## Scripts

- `run_all.sh` — benchmark runner, filtering, report generation
- `profile_bench.sh` — uftrace-based per-function profiling and flame graph data

## Categories

| Category | Framework | Source | Output |
|---|---|---|---|
| `compiler` | Criterion (Rust) | `compiler/benches/compiler_bench.rs` | `compiler_bench.json` |
| `ringbuf` | Google Benchmark | `benches/ringbuf_bench.cpp` | `ringbuf_bench.json` |
| `timer` | Google Benchmark | `benches/timer_bench.cpp` | `timer_bench.json` |
| `thread` | Google Benchmark | `benches/thread_bench.cpp` | `thread_bench.json` |
| `e2e` | Google Benchmark | `benches/e2e_bench.cpp` | `e2e_bench.json` |
| `pdl` | pcc + runtime stats | `benches/pdl/*.pdl` | `pdl_bench.txt` |
| `profile` | uftrace | `benches/profile_bench.sh` | `profile/` |

`all` runs every category (default when no `--filter` specified).

## Usage

```bash
# Run all benchmark categories
./run_all.sh

# Run selected categories only
./run_all.sh --filter timer --filter thread

# Run benchmarks and generate markdown report
./run_all.sh --report --output-dir /path/to/output

# Convert existing JSON to markdown report (no benchmark run)
./run_all.sh --report --json /path/to/json_or_dir --output-dir /path/to/output
```

## Report Output

Default report path: `./benchmark_report.md` (current directory, or `--output-dir` if provided).

If `--json` is not specified, report input JSON files are discovered from the benchmark output directory (`benches/results` by default, or `--output-dir`).

## Build

No separate CMakeLists.txt. C++ benchmarks are compiled inline by `run_all.sh`:

```
c++ -std=c++20 -O3 -march=native -DNDEBUG \
    -I runtime/libpipit/include \
    -I runtime/libpipit/include/third_party \
    -I examples \
    <source.cpp> -lbenchmark -lpthread -o <exe>
```

PDL benchmarks: `pcc` compiles `.pdl` → `.cpp`, then the generated C++ is compiled and executed with runtime statistics.

## KPI Mapping

- `compiler` — compile-time and phase-scaling metrics
- `ringbuf` — shared-buffer throughput and contention/backpressure metrics
- `timer` — jitter/overrun and batching (K-factor) behavior
- `thread` — task deadline miss rate and scaling behavior
- `e2e` — pipeline max throughput (CPU-bound) and socket loopback
- `pdl` — end-to-end task stats from generated runtime executables
- `profile` — per-function timing via uftrace instrumentation

## Compiler Bench KPIs

`compiler/benches/compiler_bench.rs` is organized into KPI groups:

- `kpi/parse_latency` — parser latency on representative programs
- `kpi/full_compile_latency` — full compile latency (parse → resolve → graph → analyze → schedule → codegen)
- `kpi/phase_latency/*` — per-phase latency breakdown on a non-trivial pipeline
- `kpi/parse_scaling` — parser scalability vs number of tasks

## PDL Benchmarks

`benches/pdl/` contains representative pipelines:

- `simple.pdl` — single-task baseline
- `modal.pdl` — CSDF mode switching
- `multitask.pdl` — multi-task shared buffer communication
- `sdr_receiver.pdl` — multi-rate chain (FFT, FIR, decimation, shared buffers)
