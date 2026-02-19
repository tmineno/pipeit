# Pipit

A domain-specific language for describing clock-driven, real-time data pipelines using Synchronous Dataflow (SDF) semantics on shared memory.

**⚠️ Work in Progress**: Pipit is under active development. The core compiler pipeline and runtime are functional, and performance/stdlib expansion is in progress. See [TODO.md](doc/TODO.md) for the development roadmap.

## What it does

- Define actors in C++ with static input/output token rates
- Describe pipelines in `.pdl` files with a concise pipe-based syntax
- Compile to native executables via C++ code generation

```text
source.pdl → pcc → source_gen.cpp → g++/clang++ → executable
```

## Example

```
const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]
param gain = 1.0

clock 10MHz capture {
    adc(0) | mul($gain) | fft(256) | :raw | fir(coeff) | ?filtered -> signal
    :raw | mag() | stdout()
}

clock 1kHz drain {
    @signal | decimate(10000) | csvwrite("output.csv")
}
```

## Quick Start

```bash
# Build the compiler (from repo root)
cargo build --release -p pcc

# Compile a pipeline to C++
target/release/pcc examples/gain.pdl \
  -I runtime/libpipit/include/std_actors.h \
  --emit cpp -o gain.cpp

# Build the executable manually
c++ -std=c++20 -O2 gain.cpp -I runtime/libpipit/include -lpthread -o gain

# Or compile directly to executable (one step, requires C++20)
target/release/pcc examples/gain.pdl \
  -I runtime/libpipit/include/std_actors.h \
  --cflags=-std=c++20 \
  -o gain

# Run with duration and stats
./gain --duration 10s --stats
```

## Features

### Language

- Multi-rate pipelines with automatic SDF balance solving
- Fork (`:tap`), probe (`?name`), and feedback loops (`delay`)
- Inter-task shared buffers (`->` / `@`) with lock-free ring buffers
- Modal tasks with `control` / `mode` / `switch` for runtime mode switching
- Reusable sub-pipelines via `define`
- Compile-time constants and runtime parameters (`$param`)

### Compiler (`pcc`)

- Full pipeline: parse, resolve, graph, analyze, schedule, codegen
- Type checking, SDF balance verification, feedback delay validation
- Overrun policies: `drop`, `slip`, `backlog`
- Actor error propagation with structured exit codes (0/1/2)
- Diagnostic hints for common errors

### Runtime

- `--duration` with time suffixes (`10s`, `1m`, `inf`)
- `--param name=value` for runtime parameter override
- `--stats` for per-task timing statistics
- `--probe <name>` / `--probe-output <path>` for data observation
- `--release` strips probes to zero cost
- Adaptive spin-wait timer with EWMA calibration (ADR-014)

### Standard Actors (31 actors)

- **I/O**: `stdin`, `stdout`, `stderr`, `stdout_fmt`, `binread`, `binwrite`
- **Math**: `constant`, `mul`, `add`, `sub`, `div`, `abs`, `sqrt`, `threshold`
- **Statistics**: `mean`, `rms`, `min`, `max`
- **DSP**: `fft`, `c2r`, `mag`, `fir`, `delay`, `decimate`
- **Waveform generators**: `sine`, `square`, `sawtooth`, `triangle`, `noise`, `impulse`
- **External I/O**: `socket_write`, `socket_read` (UDP/IPC via [PPKT protocol](doc/spec/ppkt-protocol-spec-v0.2.x.md))

### Tools

- **pipscope** — Real-time oscilloscope GUI (ImGui + ImPlot) receiving PPKT packets via UDP

### Visualization

- `--emit graph-dot` — Graphviz DOT dataflow graph
- `--emit timing-chart` — Mermaid Gantt scheduling diagram
- `--emit schedule` — PASS firing order

## Project Structure

```
compiler/       Rust compiler (pcc)
  src/            parse → resolve → graph → analyze → schedule → codegen
  tests/          unit + integration + end-to-end coverage
runtime/        C++ runtime library (libpipit)
  libpipit/       Ring buffer, timer, statistics, networking (PPKT)
  tests/          C++ unit tests for actors and runtime components
tools/          Standalone tools
  pipscope/       Real-time oscilloscope GUI (ImGui + ImPlot)
examples/       Example .pdl files and actor headers
doc/            Language spec, ADRs, usage guide, performance report
benches/        Performance benchmarks (compiler, runtime, E2E)
```

## Documentation

- [Language Spec](doc/spec/pipit-lang-spec-v0.2.x.md)
- [PPKT Protocol Spec](doc/spec/ppkt-protocol-spec-v0.2.x.md)
- [Standard Library Reference](doc/spec/standard-library-spec-v0.2.x.md)
- [pcc Usage Guide](doc/pcc-usage-guide.md)
- [Performance Analysis Report](doc/performance-analysis-report.md)
- [Development TODO](doc/TODO.md)

## License

See repository root for license information.
