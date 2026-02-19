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
- [x] Doxygen docs + auto-generated `standard-library-spec-v0.2.x.md` (pre-commit hook)

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

## v0.2.3 - Type System Ergonomics (Polymorphism)

**Goal**: Remove duplicated actor implementations caused by wire-type variation and reduce explicit type plumbing in PDL.

### Phase 1: Spec & Design (must complete first)

- [x] **Actor polymorphism model** (lang-spec §3.5, §4.1, §10 BNF):
  - [x] Define generic actor call syntax (`actor<float>(...)`)
  - [x] Define inferred call form (`actor(...)` with type inferred from context)
  - [x] Define ambiguity rules (when explicit type args are required)
  - [x] Define compatibility with shape constraints (`actor<T>(...)[N]`)
  - [x] **Create ADR-015**: Monomorphization strategy and diagnostics policy

- [x] **Principal type inference for const/param** (lang-spec §3.3):
  - [x] Infer principal numeric type from initializer and usage constraints
  - [x] Keep explicit override syntax for future compatibility
  - [x] Specify mixed numeric literal resolution in arrays and call arguments

- [x] **Implicit conversions (safe widening only)** (lang-spec §3.4):
  - [x] Allow `int8 -> int16 -> int32 -> float -> double` and `cfloat -> cdouble`
  - [x] Keep lossy / semantic conversions explicit (`double -> float`, `cfloat -> float`, etc.)
  - [x] Add warnings for suspicious narrowing in explicit conversions (lang-spec §3.4)

### Phase 2: Compiler Implementation

- [ ] **Frontend updates**:
  - [ ] Parser/AST support for generic actor calls
  - [ ] Resolver support for polymorphic actor symbols
  - [ ] Generic argument validation and arity checks

- [ ] **Type engine updates**:
  - [ ] Constraint collection + unification for actor I/O and arguments
  - [ ] Principal type computation for `const`/`param`
  - [ ] Deterministic implicit widening insertion (safe edges only)
  - [ ] Ambiguity diagnostics with concrete fix suggestions
  - [ ] Lowering certificate generation (`Lower(G)->(G', Cert)`)
  - [ ] Obligation verifier for L1-L5 (type/rate/shape/mono/no-fallback)

- [ ] **Monomorphization + codegen**:
  - [ ] Materialize typed instances (`foo<float>`, `foo<double>`) once per program
  - [ ] Define `TypedScheduledIR` as single downstream contract
  - [ ] Ensure schedule/analysis/codegen consume the same typed graph
  - [ ] Make codegen syntax-directed from IR (no re-inference / no fallback typing)
  - [ ] Preserve existing runtime/ABI behavior for non-generic actors

- [ ] **Tests**:
  - [ ] Positive/negative type inference tests
  - [ ] Ambiguity and mismatch diagnostic golden tests
  - [ ] Codegen compile tests for multiple instantiations per actor

---

## v0.3.0 - Language Evolution

**Goal**: Improve PDL ergonomics based on real usage experience. Design-first approach.

> **Note**: Type inference, polymorphism, and safe widening have been moved to v0.2.3 and are specified in lang-spec §3.3–§3.5 and pcc-spec §9.2. This milestone now covers remaining language evolution items.

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
  - [ ] Collect real-world examples and pain points from v0.2.3 usage

---

## v0.3.x - Ecosystem & Quality of Life

**Goal**: Make Pipit easier to use and deploy in real projects.

### Standard Actor Library Expansion (migrated from former v0.2.3)

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

- [ ] **Actor header organization**:
  - [ ] Split `actors.h` into categories: `io.h`, `filters.h`, `math.h`, etc.
  - [ ] Maintain `actors.h` as umbrella include
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

## v0.4.0 - Advanced Features (Future)

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

## v0.5.0 - Production Hardening (Future)

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

- **v0.2.3** Phase 1 spec/design complete; Phase 2 compiler implementation is next
- **v0.3.x** now includes former v0.2.3 stdlib expansion backlog
- **v0.3.0** covers remaining language evolution after v0.2.3 type system work
- **v0.4.0+** deferred until core is stable and well-characterized
- Performance characterization should inform optimization priorities (measure before optimizing)
- Spec files renamed to versioned names (`pipit-lang-spec-v0.2.x.md`, `pcc-spec-v0.2.x.md`); version tracked in file header
