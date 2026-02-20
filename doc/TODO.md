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

**Goal**: Remove duplicated actor implementations caused by wire-type variation and reduce explicit type plumbing in PDL.

### Phase 1: Spec & Design ✅

- [x] **Actor polymorphism model** (lang-spec §3.5, §4.1, §10 BNF):
  - [x] Define generic actor call syntax (`actor<float>(...)`)
  - [x] Define inferred call form (`actor(...)` with type inferred from context)
  - [x] Define ambiguity rules (when explicit type args are required)
  - [x] Define compatibility with shape constraints (`actor<T>(...)[N]`)
  - [x] **Create ADR-016**: Monomorphization strategy and diagnostics policy

- [x] **Principal type inference for const/param** (lang-spec §3.3):
  - [x] Infer principal numeric type from initializer and usage constraints
  - [x] Keep explicit override syntax for future compatibility
  - [x] Specify mixed numeric literal resolution in arrays and call arguments

- [x] **Implicit conversions (safe widening only)** (lang-spec §3.4):
  - [x] Allow `int8 -> int16 -> int32 -> float -> double` and `cfloat -> cdouble`
  - [x] Keep lossy / semantic conversions explicit (`double -> float`, `cfloat -> float`, etc.)
  - [x] Add warnings for suspicious narrowing in explicit conversions (lang-spec §3.4)

### Phase 2: Compiler Implementation ✅

- [x] **1) Runtime / ACTOR Macro Foundation**
  - [x] Support `template <typename T> ACTOR(name, IN(T, N), ...)` expansion as class template
  - [x] Ensure generated class template instantiates correctly via `Actor_name<float>` etc.
  - [x] Example polymorphic actors in `examples/poly_actors.h` (poly_scale, poly_pass, poly_block_pass, poly_accum)

- [x] **2) Actor Metadata Manifest Pipeline (pcc)**
  - [x] `TypeExpr` enum (`Concrete(PipitType)` / `TypeParam(String)`) for polymorphic port types
  - [x] `type_params: Vec<String>` on `ActorMeta` (empty for concrete actors)
  - [x] `actors.meta.json` schema v1 manifest loading (`load_manifest()`)
  - [x] Manifest generation from header scan (`generate_manifest()`)
  - [x] CLI flags: `--actor-meta`, `--meta-cache-dir`, `--no-meta-cache`
  - [x] Header-hash manifest cache with invalidation

- [x] **3) Frontend Updates**
  - [x] `type_args: Vec<Ident>` on `ActorCall` AST node
  - [x] `<`/`>` tokens in lexer for actor call context
  - [x] Parser: `IDENT ('<' pipit_type (',' pipit_type)* '>')? '(' args? ')' shape_constraint?`
  - [x] Resolver: polymorphic actor lookup by base name, type arg arity validation

- [x] **4) Type Engine (new `type_infer.rs` module)**
  - [x] Constraint-based type inference from actor signatures and pipe connections
  - [x] Explicit type argument resolution (`fir<float>(coeff)`)
  - [x] Inferred type argument resolution from pipe context
  - [x] Widening chain detection (`int8→...→double`, `cfloat→cdouble`)
  - [x] Monomorphization: produce concrete `ActorMeta` for each polymorphic call
  - [x] Ambiguity diagnostics with fix suggestions

- [x] **5) Typed Lowering & Verification (new `lower.rs` module)**
  - [x] Widening node insertion (synthetic `_widen_{from}_to_{to}` actors)
  - [x] Concrete actor map construction (monomorphized + original concrete)
  - [x] L1-L5 proof obligation verification with `Cert` evidence
  - [x] L1: type consistency, L2: widening safety, L3: rate/shape preservation, L4: monomorphization soundness, L5: no fallback typing

- [x] **6) Pipeline Integration & Codegen**
  - [x] New pipeline: `parse → resolve → type_infer → lower_verify → graph → analyze → schedule → codegen`
  - [x] `codegen_with_lowered()` consumes `LoweredProgram` for template instantiation
  - [x] `Actor_name<float>` template syntax in generated C++
  - [x] `lookup_actor()` prefers lowered concrete metadata over raw registry
  - [x] Full backward compatibility: non-polymorphic programs unchanged

- [x] **7) Tests**
  - [x] 458 tests passing (344 unit + 108 integration + 6 runtime)
  - [x] Manifest loading/generation tests
  - [x] Template actor header scanning tests
  - [x] Parser tests for `actor<type>(...)` syntax
  - [x] Resolver tests for polymorphic actor lookup
  - [x] Type inference unit tests (explicit + inferred + widening)
  - [x] L1-L5 verification unit tests (pass + fail cases)
  - [x] Codegen template instantiation syntax tests
  - [x] Integration tests: polymorphic PDL → C++ → compile

### Deferred to follow-up

- [ ] Narrowing conversion warnings (SHOULD-level, lang-spec §3.4)
- [ ] Comprehensive golden test suite (full type matrix coverage)
- [ ] Diagnostic polish (multi-line error context, candidate suggestions)

---

## v0.3.1 - Codegen Correctness & Throughput Hardening

**Goal**: Fix implementation-side regressions in shape/dimension resolution and shared-buffer codegen, then lock behavior with targeted tests.

- [x] **Fix dimension inference precedence for symbolic actor params (e.g., `fir(coeff)`)**
  - [x] Treat span-derived dimension inference as a first-class resolved source when deciding whether shape is unresolved
  - [x] Prevent reverse shape propagation from overriding already-resolved symbolic dimensions
  - [x] Add explicit mismatch diagnostics when inferred dimension value conflicts with explicit arg/shape constraint
  - [x] Verify generated actor params preserve stdlib semantics (`fir(coeff)` uses `N = len(coeff)` unless explicitly constrained)

- [x] **Fix shared-buffer I/O granularity in codegen**
  - [x] Stop modeling inter-task `BufferRead`/`BufferWrite` as effectively one-token firings in emitted loops
  - [x] Emit block ring-buffer operations whenever schedule information allows (`read(..., count>1)`, `write(..., count>1)`)
  - [x] Keep fail-fast + retry semantics intact while reducing retry-loop frequency

- [x] **Reduce actor construction overhead in hot loops**
  - [x] Hoist actor object construction out of per-firing inner loops when semantics permit
  - [x] Keep runtime-parameter update behavior correct at iteration boundaries
  - [x] Add a clear policy for actors that cannot be safely hoisted

- [x] **Add regression tests for these issues**
  - [x] `analyze` tests: symbolic dimension resolution prefers explicit args/span-derived values over propagated shape when both exist
  - [x] `schedule`/`codegen` tests: generated FIR call sites do not emit out-of-bounds pointer strides for `fir(coeff)` pipelines
  - [x] `codegen` tests: shared-buffer I/O emits block-size ring-buffer ops for multi-token edges
  - [x] `codegen` tests: actor construction count in generated C++ is hoisted (no per-firing temporary construction where hoistable)
  - [x] Integration test: `examples/example.pdl` compile/codegen smoke assertion for safe FIR indexing and stable shared-buffer transfer shape

- [x] **Unify node port-rate resolution in `analyze` and remove duplicate rate inference in downstream phases**
  - [x] Add precomputed `node_port_rates` to analysis result
  - [x] Consume analysis-owned rates in `schedule` edge-buffer sizing
  - [x] Consume analysis-owned rates in `codegen` actor firing stride resolution

- [x] **Runtime performance verification (2026-02-20)**
  - [x] Compare `b06071d` vs `5842279` on `BM_E2E_PipelineOnly` (5 reps, median) with no clear regression (within measurement noise)
  - [x] Compare generated-PDL runtime stats (`--filter pdl`) with no systematic throughput degradation
  - [ ] ~~Re-run socket-loopback benchmark after local port-bind issue (`localhost:19876`) is resolved~~ (deferred — port-bind infra issue)

- [x] **Follow-up simplification**
  - [x] Collapse remaining dimension-parameter fallback duplication (`analyze`/`codegen`) into a single analysis-owned artifact (`dim_resolve.rs`)

---

## v0.3.2 - Polymorphic Standard Actors & Library Split

**Goal**: Apply v0.3.0 polymorphism to standard actor library; begin modular header organization.

- [x] **Make 11 standard actors polymorphic** (`template<typename T>`):
  - [x] Source actors: `constant`, `sine`, `square`, `sawtooth`, `triangle`, `noise`, `impulse`
  - [x] I/O actors: `stdout`, `stderr`, `stdin`, `stdout_fmt`
  - [x] Update doc comments with "Polymorphic: works with any numeric wire type."

- [x] **Split arithmetic actors into `std_math.h`**:
  - [x] Extract 7 actors (`mul`, `add`, `sub`, `div`, `abs`, `sqrt`, `threshold`) from `std_actors.h`
  - [x] Add `#include <std_math.h>` to `std_actors.h` for C++ backward compatibility
  - [x] Update all compiler test registries and bench to load `std_math.h`

- [x] **Unify test include paths**:
  - [x] Integration tests use `-I runtime/libpipit/include/` (directory) instead of individual headers
  - [x] Runtime C++ tests updated with explicit `<float>` template params

- [x] **Fix test regressions from polymorphic changes**:
  - [x] 4 unit tests: update type-check tests for polymorphic actors (analysis skips polymorphic edges)
  - [x] 8 integration tests: add explicit `<float>` annotations for source actors where T cannot be inferred
  - [x] 16 runtime C++ tests: add `<float>` template params to actor instantiations

- [x] **Verify compiler error assertion coverage**:
  - [x] Analysis phase: concrete-to-concrete type mismatch (e.g., `fft | fft`) still caught
  - [x] Type inference phase: ambiguous polymorphic calls (e.g., `sine() | stdout()`) correctly diagnosed
  - [x] Polymorphic stdout accepts cfloat from fft (valid — no false error)

- [x] 358 unit + 112 integration + 6 runtime tests passing

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
