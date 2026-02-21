# Pipit Benchmarks

`./run_all.sh` is the only script in this directory.

It provides three minimal functions:

1. Filtered benchmark execution
2. Report generation
3. JSON -> Markdown conversion

## Categories

- `compiler`
- `ringbuf`
- `timer`
- `thread`
- `pdl`
- `all`

## Usage

```bash
# Run all benchmark categories
./run_all.sh

# Run selected categories only
./run_all.sh --filter timer --filter thread

# Run benchmark and generate markdown report to current directory
./run_all.sh --report

# Run benchmark and generate markdown report to specified directory
./run_all.sh --report --output-dir /path/to/output

# Convert JSON to markdown report (no benchmark run), output to specified directory
./run_all.sh --report --json /path/to/json_or_dir --output-dir /path/to/output

# Convert JSON to markdown report (no benchmark run), output to current directory
./run_all.sh --report --json /path/to/json_or_dir
```

## Report Output

Default report path:

- `./benchmark_report.md` (current directory)

If `--json` is not specified, report input JSON files are discovered from benchmark output directory (`benches/results` by default, or `--output-dir` if provided).

## KPI Mapping

- `compiler`: compile-time and phase-scaling metrics
- `ringbuf`: shared-buffer throughput and contention/backpressure metrics
- `timer`: jitter/overrun and batching (`K-factor`) behavior
- `thread`: task deadline miss rate and scaling behavior
- `pdl`: end-to-end task stats from generated runtime executables

## Compiler Bench KPIs

`compiler/benches/compiler_bench.rs` is organized into KPI groups:

- `kpi/parse_latency`: parser latency on representative programs
- `kpi/full_compile_latency`: full compile latency (`parse -> resolve -> graph -> analyze -> schedule -> codegen`)
- `kpi/phase_latency/*`: per-phase latency breakdown on a non-trivial pipeline
- `kpi/parse_scaling`: parser scalability vs number of tasks

## Stable Compiler Measurements

Use `./compiler_bench_stable.sh` for reproducible compiler KPI runs.

What it enforces:

- CPU pinning via `taskset`
- fixed Criterion timing knobs
- optional sequential baseline-vs-current runs via `git worktree`

Examples:

```bash
# Full compile KPI set on pinned CPU
./compiler_bench_stable.sh

# Single KPI
./compiler_bench_stable.sh --filter 'kpi/full_compile_latency/complex'

# A/B against baseline ref
./compiler_bench_stable.sh --baseline-ref HEAD
```

`--baseline-ref` mode uses a shared cargo target dir plus Criterion
`--save-baseline/--baseline`, so the reported `change:` numbers are true A/B.
