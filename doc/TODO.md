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

### Deferred to v0.5.x

- [x] Deferred to v0.5.x: Narrowing conversion warnings (SHOULD-level, lang-spec §3.4)
- [x] Deferred to v0.5.x: Comprehensive golden test suite (full type matrix coverage)
- [x] Deferred to v0.5.x: Diagnostic polish (multi-line error context, candidate suggestions)

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
- [x] Deferred to v0.5.x+: Socket-loopback benchmark (port-bind infra issue)

## v0.3.2 - Polymorphic Standard Actors & Library Split

- [x] Make 11 standard actors polymorphic (`constant`, `sine`, `square`, `sawtooth`, `triangle`, `noise`, `impulse`, `stdout`, `stderr`, `stdin`, `stdout_fmt`)
- [x] Split 7 arithmetic actors into `std_math.h` (backward-compatible via `#include`)
- [x] Fix test regressions: 4 unit + 8 integration + 16 runtime C++ tests updated
- [x] Verify error coverage: concrete type mismatch, ambiguous polymorphic, cfloat-through-fft

---

## v0.3.3 - Compiler Refactor Strategy (Analyze/Codegen) ✅

**Goal**: Reduce algorithmic bottlenecks and maintenance cost in compiler hot paths after v0.3.2 merge.

- [x] Completed scope: graph index layer, analyze worklist propagation, codegen decomposition, and dimension-override safety hardening.
- [x] Outcome: deterministic behavior/output retained with no correctness or benchmark regressions.
- [x] Exit summary: ~50% NLOC/complexity reduction on prioritized hotspots in `compiler/src/codegen.rs` (`emit_task_functions`, `emit_firing`, `emit_actor_firing`) and `compiler/src/analyze.rs` (shape propagation/tracing path).

---

## v0.3.4 - Compiler Performance Follow-up (Partially Completed; remainder deferred to v0.5.x+)

**Goal**: Improve end-to-end compile latency using profiler-guided optimization after v0.3.3 refactor.

- [x] Completed in v0.3.4: measurement hardening (`benches/compiler_bench_stable.sh`, profiling notes, CI A/B baseline artifacts) and safe-first intra-task branch optimization (single-thread loop fusion across `Fork/Probe` passthrough chains).
- [x] Deferred to v0.5.x+: remaining hotspot optimization, type-inference allocation/clone reduction, registry/header-load caching, KPI exit validation, and task-internal branch parallelization study.
- [x] Detailed deferred items are tracked under `v0.5.x` -> `Deferred Backlog from v0.3.x`.

---

## v0.4.0 - Compiler Architecture Rebuild (IR Unification + Pass Manager)

**Goal**: Perform a large architecture transition so all downstream phases consume one typed/lowered IR contract, pass execution is dependency-driven, and backend/runtime responsibilities are clearly separated.

> **Execution discipline**: each major phase is split into (1) mechanical reshape, (2) behavior-locked change with tests, (3) optimization/cleanup.

### Baseline Snapshot (2026-02-21)

- Compiler core footprint (`main/resolve/type_infer/lower/graph/analyze/schedule/codegen/registry/dim_resolve/subgraph_index`): **16,986 LOC**
- Major hotspot files: `codegen.rs` (4,152 LOC), `analyze.rs` (3,264 LOC), `registry.rs` (1,683 LOC), `graph.rs` (1,738 LOC), `resolve.rs` (1,562 LOC)
- Structural drift to resolve: `graph/analyze/schedule` still consume AST+resolved while `codegen` additionally consumes lowered typed artifacts

### Phase 0: Spec/ADR Contract Freeze (Design Gate)

- [x] Publish `doc/spec/pcc-spec-v0.4.0.md` with explicit IR boundaries (`AST -> HIR -> THIR -> LIR`) and pass ownership
- [x] Add ADR: pass manager architecture, artifact model, and cache invalidation rules (`doc/adr/020-pass-manager-artifact-model-and-invalidation.md`)
- [x] Add ADR: stable semantic IDs (replace span-keyed semantic maps with stable IDs for identity) (`doc/adr/021-stable-semantic-ids-over-span-keys.md`)
- [x] Add ADR: diagnostics data model (`code`, primary/secondary spans, cause chain, machine-readable payload) (`doc/adr/022-unified-diagnostics-model-with-cause-chain.md`)
- [x] Backward-compatibility decision gate: keep v0.3 language/CLI behavior unless marked as explicit breaking change (`doc/adr/023-v040-backward-compatibility-gate.md`, `doc/spec/pcc-spec-v0.4.0.md`)

### Phase 1: Mechanical Foundations (No Behavior Change)

- [ ] Introduce stable IDs for calls/nodes/definitions and thread them through resolve/type/lower/graph/analyze/schedule/codegen
- [ ] Remove span-as-primary-key usage from semantic tables (`HashMap<Span, ...>` -> stable-ID keyed maps)
- [ ] Centralize graph traversal helpers (`subgraphs`, node/edge lookup, back-edge detection) to remove duplicated local implementations
- [ ] Add shared program query helpers for `set`/task lookups currently duplicated across phases
- [x] Lock behavior with snapshot tests to guarantee byte-equivalent output before semantic changes (insta snapshots for 7 examples, codegen determinism fix)

### Phase 2: IR Unification (Behavior Change, Diff Locked)

- [ ] Introduce HIR normalization pass (define expansion strategy, modal normalization, tap/buffer explicitness)
- [ ] Extend current lowering output into THIR that is complete enough for graph/analyze/schedule (monomorphized actors + explicit widening + shape/rate constraints)
- [ ] Migrate `graph`, `analyze`, and `schedule` to consume THIR instead of rebuilding semantics from AST+resolve
- [ ] Introduce LIR for backend consumption (finalized schedule, buffer layout, concrete actor instantiations, conversion nodes)
- [ ] Restrict backend to syntax-directed emission from LIR (no fallback type/rate/dim inference in codegen)

### Phase 3: Pass Manager + Artifact/Caching Layer

- [ ] Implement pass registry with declared inputs, outputs, invariants, and invalidation keys
- [ ] Compute minimal pass subset for each `--emit` target (avoid unnecessary full pipeline execution)
- [ ] Add artifact hashing and reusable cache for heavy phases (registry/type/analysis/schedule artifacts)
- [ ] Integrate manifest/header provenance into cache keys and diagnostics
- [ ] Keep deterministic ordering and reproducible outputs across machines/CI

### Phase 4: Verification Framework Generalization

- [ ] Generalize lower-only `Cert` model into stage-scoped verification framework
- [ ] Add `verify_hir`, `verify_thir`, `verify_schedule`, and `verify_lir` passes with explicit obligations
- [ ] Promote proof obligations to CI gates (debug + release test matrix)
- [ ] Add regression corpus for known failure classes (type mismatch lineage, shape/rate contradictions, invalid schedule states)
- [ ] Keep existing L1-L5 guarantees as a strict subset of the new framework

### Phase 5: Diagnostics Architecture Upgrade

- [ ] Introduce unified diagnostic payload: `code`, `level`, `message`, primary span, related spans, hint, cause chain
- [ ] Record provenance through type + shape constraint solving to explain contradiction paths
- [ ] Add stable diagnostic codes and compatibility policy for tests/tooling
- [ ] Add machine-readable diagnostics mode (JSON) while preserving current human-readable output
- [ ] Improve ambiguity and mismatch diagnostics with candidate and remediation suggestions

### Phase 6: Backend/Runtime Boundary Refactor

- [ ] Move generic runtime shell logic (CLI parsing, probe init, duration wait, stats printing, thread launch policy) from generated C++ into runtime API
- [ ] Make codegen emit compact program data/config + actor wiring rather than large hand-built runtime scaffolding
- [ ] Keep runtime behavior compatibility for `--duration`, `--param`, `--probe`, `--probe-output`, `--stats`
- [ ] Reduce generated C++ volume and compile overhead by eliminating duplicated boilerplate emission
- [ ] Preserve deterministic generated symbol layout for test stability

### Phase 7: Registry Determinism and Hermetic Build Inputs

- [ ] Promote `actors.meta.json` to first-class compiler input path for deterministic builds
- [ ] Treat header scanning as explicit metadata generation workflow (separate from core semantic compile path)
- [ ] Define deterministic overlay and precedence rules for manifest/header hybrid flows
- [ ] Add provenance stamping (input manifest hash, schema version, header set hash) to emitted artifacts
- [ ] Add CI reproducibility tests (same inputs -> same artifacts/diagnostics)

### Phase 8: Test Strategy and Migration Hardening

- [ ] Introduce IR-level golden tests for HIR/THIR/LIR snapshots
- [ ] Add differential pipeline tests (legacy path vs unified path) before old path removal
- [ ] Expand property/fuzz tests for parser->HIR + constraint solver + scheduler invariants
- [ ] Add migration guide for contributors (new pass boundaries, where to add checks/tests)
- [ ] Keep full matrix green (format, lint, typecheck, unit/integration/runtime tests)

### Exit Criteria

- [ ] Downstream phases (`graph/analyze/schedule/codegen`) consume unified typed/lowered IR contract in production path
- [ ] Pass manager resolves all `--emit` modes with minimal-pass evaluation
- [ ] Duplicate helper/inference logic is removed from per-phase local implementations
- [ ] Compiler core footprint reduced by >=25% from baseline (16,986 LOC) without feature regressions
- [ ] `codegen.rs` footprint reduced by >=20% from baseline (4,152 LOC) via backend/runtime split
- [ ] No correctness regressions; no statistically significant compiler KPI regressions vs v0.3.4 baseline
- [ ] Any breaking behavior is explicitly versioned and documented in spec + ADR

---

## v0.5.x - Ecosystem & Quality of Life

**Goal**: Make Pipit easier to use and deploy in real projects.

> **Status**: Deferred. All unchecked (`- [ ]`) items in this `v0.5.x` section are deferred.

### Deferred Backlog from v0.3.x (moved pre-v0.4.0 open items)

- [ ] **From v0.3.0**
  - [ ] Narrowing conversion warnings (SHOULD-level, lang-spec §3.4)
  - [ ] Comprehensive golden test suite (full type matrix coverage)
  - [ ] Diagnostic polish (multi-line error context, candidate suggestions)

- [ ] **From v0.3.1**
  - [ ] Socket-loopback benchmark (port-bind infra issue)

- [ ] **From v0.3.4 / Priority 1: Remaining compiler hotspots**
  - [ ] `compiler/src/codegen.rs`: optimize `param_cpp_type` and literal/type conversion helpers
  - [ ] `compiler/src/analyze.rs`: optimize `record_span_derived_dims` (dedup/indexing/allocation reduction)
  - [ ] Re-profile to confirm hotspot migration after each optimization phase

- [ ] **From v0.3.4 / Priority 2: Type inference allocation/clone reduction (profile-priority)**
  - [ ] Remove or minimize remaining `ActorMeta` cloning in `type_infer` hot paths (monomorphization/result materialization path)
  - [ ] Reduce String/HashMap churn in monomorphization keys (prefer reused keys/interned forms)

- [ ] **From v0.3.4 / Priority 3: Registry + header loading costs**
  - [ ] Cache parsed header metadata across repeated invocations (hash-keyed)
  - [ ] Avoid redundant overlay work when include-set + header hashes are unchanged
  - [ ] Re-benchmark repeated single-file compiles (`simple`, `multitask`, `modal`) after cache changes

- [ ] **From v0.3.4 / Priority 4: Exit criteria validation**
  - [ ] `kpi/full_compile_latency/complex` median improved by >= 5% vs v0.3.3 baseline
  - [ ] `kpi/full_compile_latency/modal` median improved by >= 5% vs v0.3.3 baseline
  - [ ] No statistically significant regressions in analyze/codegen phase KPIs (using CI `compiler-perf-ab` trend)
  - [ ] No correctness regressions (all unit/integration/runtime tests pass)

- [ ] **From v0.3.4 / Priority 5: Task-internal branch parallelization study**
  - [ ] Define safety gate for parallel branches (side-effect-free + thread-safe actors only)
  - [ ] Add actor metadata/annotation strategy for effect/thread-safety classification
  - [ ] Specify deterministic behavior policy for sinks/probes/shared-buffer boundaries
  - [ ] Prototype runtime-context propagation (`iteration_index`, `task_rate_hz`) for branch workers
  - [ ] Only enable after benchmarked speedup and regression-free correctness validation

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
- **v0.4.0 Phase 0 ADRs**: ADR-020 (pass manager/artifact model), ADR-021 (stable semantic IDs), ADR-022 (diagnostics model), ADR-023 (backward-compatibility gate)
- **v0.3.2** applies v0.3.0 polymorphism to 11 std actors; begins modular header split (`std_math.h`)
- **v0.5.x** now includes former v0.3.0 stdlib expansion backlog
- **pre-v0.4.0 open items** were moved to `v0.5.x` backlog (`Deferred Backlog from v0.3.x`)
- **v0.4.0** now tracks the compiler architecture rebuild (IR unification, pass manager, diagnostics/verification, backend/runtime boundary refactor)
- **v0.5.x** open items are currently deferred
- Performance characterization should inform optimization priorities (measure before optimizing)
- Spec files renamed to versioned names (`pipit-lang-spec-v0.3.0.md`, `pcc-spec-v0.3.0.md`); `v0.2.0` specs are frozen from tag `v0.2.2`
