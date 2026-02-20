# Pipit Development Roadmap

**v0.1.0 Tagged** ✅ (2026-02-15)

- Full compiler pipeline (parse → resolve → graph → analyze → schedule → codegen)
- Runtime library with lock-free ring buffers, timers, statistics
- 265 passing tests, comprehensive documentation
- Basic benchmarks (compiler + runtime primitives + end-to-end PDL)

---

## v0.1.1 - Probe Completion & Hardening ✅

- [x] Probe runtime wiring (`--probe`, `--probe-output`, startup validation, exit code 2)
- [x] Release build guard (`#ifndef NDEBUG`)
- [x] End-to-end test coverage (8 tests: compile/run, stats, probe enable/disable/error)

---

## v0.1.2 - Standard Actor Library ✅

- [x] 25 standard actors (I/O, math, statistics, DSP) in `std_actors.h`
- [x] 143 total tests (85 integration + 58 C++ runtime)
- [x] Doxygen docs + auto-generated `standard-library-spec-v0.3.0.md` (pre-commit hook)

---

## v0.2.0 - Frame Dimension Inference & Vectorization Alignment ✅

- [x] PortShape model: `rate = product(shape)`, flat runtime buffers (ADR-007)
- [x] `SHAPE(...)` registry parsing, `actor(...)[d0, d1, ...]` call-site syntax
- [x] Shape-aware rate resolution in analyze/schedule/codegen
- [x] SDF edge shape inference (§13.3.3) with passthrough tracing
- [x] 5 diagnostic error types (runtime param as dim, unknown name, unresolved, conflicting, cross-clock)
- [x] 353 tests passing, full backward compatibility with v0.1-style programs

---

## v0.2.1 - Performance Characterization & Spec Sheet ✅

- [x] KPI benchmark suite (ADR-012) + performance analysis report
- [x] Scheduler/timer overhead reduction (ADR-009, K-factor batching)
- [x] Ring buffer contention optimization (ADR-010, PaddedTail, two-phase memcpy)
- [x] Adaptive spin-wait timer with EWMA calibration (ADR-014)

---

## v0.2.2 - Sink & Source Actors (External Process I/O) ✅

- [x] PPKT protocol (ADR-013) + `pipit_net.h` transport layer
- [x] `socket_write`/`socket_read` actors (non-blocking UDP/IPC, sender-side chunking)
- [x] Oscilloscope GUI (`tools/pipscope/`): ImGui + ImPlot, PPKT receiver
- [x] Waveform generators: sine, square, sawtooth, triangle, noise, impulse (6 actors)

---

## v0.2.2a - Spec Alignment & Runtime Hardening ✅

- [x] Strict parameter type checking: `is_int_literal` tracking in parser/AST, exact type match in analyze (no implicit Int→Float)
- [x] Modal switch semantics: soft-deprecate `switch ... default`, support `$param` and external buffer as ctrl sources
- [x] Ring buffer fairness: yield + retry loops replace fail-fast reads/writes in codegen
- [x] Shared buffer optimization: skip writes when no readers, static edge buffer declarations
- [x] Default memory pool: 64 MB when `set mem` not specified
- [x] uftrace-based block profiling benchmark (`profile_bench.sh`)
- [x] ADR-015: v0.2.0 spec-implementation alignment decisions

---

## v0.3.0 - Type System Ergonomics (Polymorphism) ✅

- [x] Actor polymorphism: `actor<T>(...)` syntax, inferred T from pipe context, ambiguity diagnostics (lang-spec §3.5, ADR-016)
- [x] Principal type inference for const/param (lang-spec §3.3)
- [x] Implicit safe widening: `int8→...→double`, `cfloat→cdouble` (lang-spec §3.4)
- [x] `type_infer.rs`: constraint-based inference, monomorphization, widening chain detection
- [x] `lower.rs`: widening node insertion, L1–L5 proof obligations with `Cert` evidence
- [x] Manifest pipeline: `actors.meta.json` schema v1, header-hash cache, CLI flags
- [x] Codegen: `Actor_name<float>` template instantiation, backward-compatible
- [x] 458 tests (344 unit + 108 integration + 6 runtime)

### Deferred

- [ ] Narrowing conversion warnings (SHOULD-level, lang-spec §3.4)
- [ ] Comprehensive golden test suite (full type matrix coverage)
- [ ] Diagnostic polish (multi-line error context, candidate suggestions)

---

## v0.3.1 - Codegen Correctness & Throughput Hardening

- [x] Fix dimension inference precedence for symbolic actor params (`fir(coeff)`)
- [x] Add dimension mismatch diagnostics (explicit arg / shape constraint / span-derived conflicts)
- [x] Fix shared-buffer I/O granularity — emit block ring-buffer ops
- [x] Hoist actor construction out of per-firing inner loops
- [x] Unify node port-rate resolution into analysis-owned `node_port_rates`
- [x] Extract shared dim-resolution helpers into `dim_resolve.rs`
- [x] Regression tests for all above (analyze, schedule, codegen, integration)
- [x] Runtime perf verification (no regression vs `5842279`)
- [ ] ~~Socket-loopback benchmark~~ (deferred — port-bind infra issue)

## v0.3.2 - Polymorphic Standard Actors & Library Split

- [x] Make 11 standard actors polymorphic (`constant`, `sine`, `square`, `sawtooth`, `triangle`, `noise`, `impulse`, `stdout`, `stderr`, `stdin`, `stdout_fmt`)
- [x] Split 7 arithmetic actors into `std_math.h` (backward-compatible via `#include`)
- [x] Fix test regressions: 4 unit + 8 integration + 16 runtime C++ tests updated
- [x] Verify error coverage: concrete type mismatch, ambiguous polymorphic, cfloat-through-fft

---

## v0.4.0 - Language Evolution

**Goal**: Improve PDL ergonomics based on real usage experience. Design-first approach.

> **Note**: Type inference, polymorphism, and safe widening have been moved to v0.3.0 and are specified in lang-spec §3.3–§3.5 and pcc-spec §9.2. This milestone now covers remaining language evolution items.

- [ ] **Explicit type annotations for const/param** (future ergonomics):
  - [ ] Support syntax: `const x: int32 = 42`, `param gain: double = 2.5`
  - [ ] Type checking for array elements: `[1, 2.0]` → error (mixed types)

- [ ] **Better type error messages**:
  - [ ] Show expected vs actual types in context
  - [ ] Suggest conversion actors with exact syntax
  - [ ] Trace type through pipeline
  - [ ] Highlight problematic pipe operator in source

- [ ] **DSL survey and experience report**:
  - [ ] Survey type systems in similar DSLs (GNU Radio, Faust, StreamIt)
  - [ ] Collect real-world examples and pain points from v0.3.0 usage

---

## v0.4.x - Ecosystem & Quality of Life

**Goal**: Make Pipit easier to use and deploy in real projects.

### Standard Actor Library Expansion (migrated from former v0.3.0)

#### Phase 2: Signal Processing Basics (Medium Complexity)

- [ ] **Simple filters** (MEDIUM complexity):
  - [ ] `lpf(cutoff, order)` - Low-pass filter (Butterworth)
  - [ ] `hpf(cutoff, order)` - High-pass filter
  - [ ] `notch(freq, q)` - Notch filter
  - [ ] Test with known signal characteristics

- [ ] **Basic transforms** (MEDIUM-HIGH complexity):
  - [ ] `ifft(N)` - Inverse FFT
  - [ ] `rfft(N)` - Real FFT (optimize for real input)
  - [ ] Validate against reference implementations (e.g., FFTW)

- [ ] **Windowing** (LOW-MEDIUM complexity):
  - [ ] `window(N, type)` - Window functions (hann, hamming, blackman)
  - [ ] Test window properties (energy, side lobe levels)

#### Phase 3: Advanced Signal Processing (High Complexity)

- [ ] **WAV file I/O** (MEDIUM-HIGH complexity):
  - [ ] `wavread(path)` - WAV file reader (streaming, 16/24/32-bit PCM)
  - [ ] `wavwrite(path)` - WAV file writer (configurable sample rate, channels)
  - [ ] Handle WAV header parsing, endianness

- [ ] **Advanced filters** (HIGH complexity):
  - [ ] `iir(b_coeff, a_coeff)` - IIR filter (biquad sections)
  - [ ] `bpf(low, high, order)` - Band-pass filter
  - [ ] Numerical stability testing

- [ ] **Resampling** (HIGH complexity):
  - [ ] `resample(M, N)` - Rational resampling (upsample M, downsample N)
  - [ ] `interp(N)` - Interpolation (zero-insert + LPF)
  - [ ] `downsample(N)` - Downsampling (LPF + decimate)

- [ ] **Advanced transforms** (HIGH complexity):
  - [ ] `dct(N)` - Discrete Cosine Transform
  - [ ] `hilbert(N)` - Hilbert transform (analytic signal)
  - [ ] `stft(N, hop)` - Short-Time Fourier Transform
  - [ ] `istft(N, hop)` - Inverse STFT

- [ ] **Advanced statistics** (MEDIUM complexity):
  - [ ] `var(N)` - Variance
  - [ ] `std(N)` - Standard deviation
  - [ ] `xcorr(N)` - Cross-correlation
  - [ ] `acorr(N)` - Auto-correlation
  - [ ] `convolve(N, kernel)` - Convolution

- [ ] **Control flow** (MEDIUM complexity):
  - [ ] `gate(threshold)` - Pass/block based on signal level
  - [ ] `clipper(min, max)` - Hard clipping
  - [ ] `limiter(threshold)` - Soft limiting
  - [ ] `agc(target, attack, release)` - Automatic Gain Control

#### Infrastructure & Documentation

- [ ] **Testing infrastructure**:
  - [ ] Per-actor unit test framework
  - [ ] Test harness for actor correctness
  - [ ] Edge case testing (zero, infinity, NaN)
  - [ ] Performance tests (measure ns/firing)

- [ ] **Actor documentation**:
  - [ ] API reference template
  - [ ] Usage examples for each actor
  - [ ] Performance characteristics
  - [ ] Known limitations

- [ ] **Example pipelines**:
  - [ ] Audio effects (basic filters, gain)
  - [ ] SDR examples (if filters/transforms complete)
  - [ ] Simple sensor processing

- [ ] **Actor header organization** (started in v0.3.2):
  - [x] Split `std_math.h` from `std_actors.h` (arithmetic actors)
  - [ ] Split remaining categories: `io.h`, `filters.h`, etc.
  - [x] Maintain `std_actors.h` as umbrella include (via `#include <std_math.h>`)
  - [ ] Consider `--actor-path` for automatic discovery

#### Performance & Benchmarking (deferred from v0.2.1)

- [ ] **Benchmark automation**:
  - [ ] Regression detection (statistical comparison with baseline)
  - [ ] CI integration (benchmark on merge to main)
  - [ ] Flamegraph integration, build mode assertions, ASLR control
- [ ] **Performance tuning guide**:
  - [ ] CPU affinity / NUMA placement guidance
  - [ ] Compiler optimization flags documentation
- [ ] **Extended testing**:
  - [ ] Long-running 24-hour drift test
  - [ ] Comparison with alternatives (GNU Radio, hand-coded C++)

### Runtime Improvements

- [ ] Round-robin scheduler with thread pools
- [ ] Platform support (macOS, Windows native)
- [ ] LSP server for IDE integration
- [ ] Improved documentation and tutorials

### Build & Distribution

- [ ] CMake integration for actor library
- [ ] Install target (copy headers to system include path)
- [ ] pkg-config support
- [ ] Package manager for actor distribution

---

## v0.5.0 - Advanced Features (Future)

**Goal**: Compiler optimizations, real-time scheduling, heterogeneous execution.

### Compiler Optimizations

- [ ] Fusion (merge adjacent actors)
- [ ] Constant propagation
- [ ] Dead code elimination
- [ ] Actor inlining

### Real-time Scheduling

- [ ] Priority-based scheduling
- [ ] Deadline guarantees
- [ ] CPU affinity control
- [ ] NUMA-aware placement

### Heterogeneous Execution

- [ ] GPU support (CUDA, OpenCL)
- [ ] FPGA code generation
- [ ] Accelerator offload

### Distributed Computing

- [ ] Distributed pipelines across nodes
- [ ] Network-transparent buffers
- [ ] Fault tolerance and checkpointing

---

## v0.6.0 - Production Hardening (Future)

**Goal**: Observability, reliability, security, verification for production deployments.

### Observability

- [ ] Metrics and monitoring (Prometheus, Grafana, OpenTelemetry)
- [ ] Built-in profiler and debugger
- [ ] Distributed tracing

### Reliability

- [ ] Fault tolerance
- [ ] State checkpointing and recovery
- [ ] Graceful degradation

### Security

- [ ] Sandboxing
- [ ] Input validation
- [ ] Resource limits

### Verification

- [ ] Property-based testing
- [ ] Formal verification of scheduling
- [ ] Model checking for deadlock freedom

---

## Notes

- **v0.2.2a** Spec/runtime alignment merged from `review/spec`; strict types and modal state fixes establish foundation for Phase 2
- **v0.3.0** Complete — polymorphism, type inference, monomorphization, lowering verification (L1-L5), template codegen; 458 tests passing
- **New modules**: `type_infer.rs` (constraint-based type inference), `lower.rs` (typed lowering + L1-L5 verification)
- **New pipeline**: `parse → resolve → type_infer → lower_verify → graph → analyze → schedule → codegen`
- **ADR numbering**: ADR-015 = spec alignment (from review/spec), ADR-016 = polymorphism & safe widening, ADR-017 = analysis-owned node port-rate resolution
- **v0.3.2** applies v0.3.0 polymorphism to 11 std actors; begins modular header split (`std_math.h`)
- **v0.4.x** now includes former v0.3.0 stdlib expansion backlog
- **v0.4.0** covers remaining language evolution after v0.3.0 type system work
- **v0.5.0+** deferred until core is stable and well-characterized
- Performance characterization should inform optimization priorities (measure before optimizing)
- Spec files renamed to versioned names (`pipit-lang-spec-v0.3.0.md`, `pcc-spec-v0.3.0.md`); `v0.2.0` specs are frozen from tag `v0.2.2`
