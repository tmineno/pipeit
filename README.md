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

```rust
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
c++ -std=c++20 -O2 gain.cpp \
  -I runtime/libpipit/include \
  -I runtime/libpipit/include/third_party \
  -lpthread -o gain

# Or compile directly to executable (one step, requires C++20)
target/release/pcc examples/gain.pdl \
  -I runtime/libpipit/include/std_actors.h \
  --cflags="-std=c++20 -O2" \
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

- Full pipeline: parse → resolve → HIR → type_infer → lower → graph → analyze → schedule → LIR → codegen
- Pass-manager orchestration with `--emit`-driven minimal evaluation
- Polymorphic actors with constraint-based type inference (`actor<T>`)
- Implicit safe widening: `int8 → int16 → int32 → float → double`, `cfloat → cdouble`
- Dimension mismatch diagnostics (explicit arg vs shape constraint vs span-derived conflicts)
- SDF balance verification, feedback delay validation
- Structured diagnostics (`human` / `json`) with stable diagnostic codes
- LIR edge memory classification (`Local`/`Shared`/`Alias`) with cache-line-aligned buffers

### Runtime

- `--duration` with time suffixes (`10s`, `1m`, `inf`)
- `--param name=value` for runtime parameter override
- `--stats` for per-task timing statistics
- `--probe <name>` / `--probe-output <path>` for data observation
- `--threads <n>` advisory runtime hint
- `--release` strips probes to zero cost
- `--experimental` enables experimental codegen features (reserved)
- Adaptive spin-wait timer with EWMA calibration (ADR-014)
- SPSC ring buffer specialization for single-reader inter-task buffers (ADR-029)

### Standard Actors

- Split headers: `std_actors.h`, `std_math.h`, `std_sink.h`, `std_source.h`
- **I/O**: `stdin`, `stdout`, `stderr`, `stdout_fmt`, `binread`, `binwrite`
- **Math**: `constant`, `mul`, `add`, `sub`, `div`, `abs`, `sqrt`, `threshold`, `convolution`
- **Statistics**: `mean`, `rms`, `min`, `max`
- **DSP**: `fft`, `c2r`, `mag`, `fir`, `delay`, `decimate` (PocketFFT + xsimd SIMD vectorization)
- **Waveform generators**: `sine`, `square`, `sawtooth`, `triangle`, `noise`, `impulse`
- **External I/O**: `socket_write`, `socket_read` (UDP/IPC via [PPKT protocol](doc/spec/ppkt-protocol-spec-v0.3.0.md))

### Tools

- **pipscope** — Real-time oscilloscope GUI (ImGui + ImPlot) receiving PPKT packets via UDP

### Output Modes

- `--emit exe` — Generate and compile executable (default)
- `--emit cpp` — Emit generated C++ source
- `--emit ast` — Dump parsed AST
- `--emit graph` — Dump analyzed graph view
- `--emit manifest` — Actor metadata JSON (hermetic builds, no `.pdl` required)
- `--emit build-info` — Provenance JSON (source hash, registry fingerprint)
- `--emit graph-dot` — Graphviz DOT dataflow graph
- `--emit timing-chart` — Mermaid Gantt scheduling diagram
- `--emit schedule` — PASS firing order

## Project Structure

```
compiler/       Rust compiler (pcc)
  src/            parse → resolve → HIR/THIR/LIR → analyze/schedule → codegen
  tests/          unit + integration + end-to-end coverage
runtime/        C++ runtime library (libpipit)
  libpipit/       Ring buffer, timer, statistics, networking (PPKT)
  tests/          C++ unit tests for actors and runtime components
tools/          Standalone tools
  pipscope/       Real-time oscilloscope GUI (ImGui + ImPlot)
examples/       Example .pdl files and actor headers
doc/            Language spec, ADRs, usage guide, performance specs
benches/        Performance benchmarks (compiler, runtime, E2E)
```

## Documentation

- [Language Spec](doc/spec/pipit-lang-spec-v0.3.0.md)
- [PPKT Protocol Spec](doc/spec/ppkt-protocol-spec-v0.3.0.md)
- [Standard Library Reference](doc/spec/standard-library-spec-v0.3.0.md)
- [pcc Usage Guide](doc/pcc-usage-guide.md)
- [pcc Performance KPI/Test Spec](doc/spec/pcc-perf-spec-v0.3.0.md)
- [Development TODO](doc/TODO.md)

## License

MIT — see [LICENSE](LICENSE).
