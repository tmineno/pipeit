# Pipit

A domain-specific language for describing clock-driven, real-time data pipelines using Synchronous Dataflow (SDF) semantics on shared memory.

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
# Build the compiler
cd compiler && cargo build --release

# Compile a pipeline to C++
cargo run -- ../examples/gain.pdl -I ../examples/actors.h --emit cpp -o gain.cpp

# Build the executable manually
c++ -std=c++20 -O2 gain.cpp -I ../runtime/libpipit/include -I ../examples -lpthread -o gain

# Or compile directly to executable (one step)
cargo run -- ../examples/gain.pdl -I ../examples/actors.h -o gain

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

### Generated Binary

- `--duration` with time suffixes (`10s`, `1m`, `inf`)
- `--param name=value` for runtime parameter override
- `--stats` for per-task timing statistics
- `--probe <name>` / `--probe-output <path>` for data observation
- `--release` strips probes to zero cost

### Visualization

- `--emit graph-dot` — Graphviz DOT dataflow graph
- `--emit timing-chart` — Mermaid Gantt scheduling diagram
- `--emit schedule` — PASS firing order

## Project Structure

```
compiler/       Rust compiler (pcc)
  src/            parse → resolve → graph → analyze → schedule → codegen
  tests/          262 tests (unit + C++ integration + end-to-end run)
runtime/        C++ runtime library (libpipit)
  libpipit/       Ring buffer, timer, statistics
examples/       Example .pdl files and actor headers
doc/            Language spec, ADRs, usage guide
```

## Documentation

- [Language Spec v0.1.0](doc/spec/pipit-lang-spec-v0.1.0.md)
- [pcc Usage Guide](doc/pcc-usage-guide.md)
- [Development TODO](doc/TODO.md)

## License

See repository root for license information.
