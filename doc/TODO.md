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
- Structural drift resolved: all semantic phases (`type_infer/lower/graph/analyze/schedule/codegen`) now consume HIR/ThirContext/LIR — no direct AST consumption remains

### Phase 0: Spec/ADR Contract Freeze (Design Gate)

- [x] Publish `doc/spec/pcc-spec-v0.4.0.md` with explicit IR boundaries (`AST -> HIR -> THIR -> LIR`) and pass ownership
- [x] Add ADR: pass manager architecture, artifact model, and cache invalidation rules (`doc/adr/020-pass-manager-artifact-model-and-invalidation.md`)
- [x] Add ADR: stable semantic IDs (replace span-keyed semantic maps with stable IDs for identity) (`doc/adr/021-stable-semantic-ids-over-span-keys.md`)
- [x] Add ADR: diagnostics data model (`code`, primary/secondary spans, cause chain, machine-readable payload) (`doc/adr/022-unified-diagnostics-model-with-cause-chain.md`)
- [x] Backward-compatibility decision gate: keep v0.3 language/CLI behavior unless marked as explicit breaking change (`doc/adr/023-v040-backward-compatibility-gate.md`, `doc/spec/pcc-spec-v0.4.0.md`)

### Phase 1: Mechanical Foundations (No Behavior Change) ✅

- [x] Lock behavior with snapshot tests to guarantee byte-equivalent output before semantic changes (insta snapshots for 7 examples, codegen determinism fix)
- [x] Centralize graph traversal helpers (`subgraphs`, node/edge lookup, back-edge detection) to remove duplicated local implementations (`subgraph_index.rs` + `GraphQueryCtx`)
- [x] Add shared program query helpers for `set`/task lookups currently duplicated across phases (`program_query.rs`)
- [x] Introduce stable IDs (`CallId`/`DefId`/`TaskId`) for calls/nodes/definitions and thread them through resolve/type/lower/graph/analyze/schedule/codegen (`id.rs`)
- [x] Remove span-as-primary-key usage from semantic tables (`HashMap<Span, ...>` → `HashMap<CallId, ...>` for all 5 semantic maps)

### Phase 2: IR Unification (Behavior Change, Diff Locked)

#### Phase 2a: HIR + ThirContext + Consumer Migration ✅

- [x] Introduce HIR normalization pass (`hir.rs`): define expansion, modal normalization, tap/buffer explicitness; AST+resolved → `HirProgram`
- [x] Build ThirContext wrapper (`thir.rs`): unified query API over HIR + resolved + typed + lowered + registry + precomputed metadata
- [x] Migrate `graph` to consume `HirProgram` (remove ~200 LOC define inlining + arg substitution)
- [x] Migrate `analyze` and `schedule` to consume `ThirContext` instead of `&Program` + `&ResolvedProgram`
- [x] Migrate dim-resolution queries into ThirContext methods (resolve_port_rate, infer_dim_param_from_span_args, span_arg_length_for_dim)
- [x] Update pipeline driver (`main.rs`) and all test/bench callers; all 500+ tests passing with byte-identical C++ output

#### Phase 2b: LIR Introduction + Codegen Migration ✅

- [x] Fix ThirContext `overrun_policy` default ("stop" → "drop") to match codegen behavior
- [x] ADR-025: LIR backend IR design decisions
- [x] Introduce LIR types (`lir.rs`): `LirProgram`, tasks, firings, actor args, edge buffers, modal/ctrl, directives
- [x] Implement LIR builder: `build_lir(thir, graph, analysis, schedule) -> LirProgram` (no `&Program` needed)
- [x] Migrate codegen globals: const/param/buffer/stats/directives read from LIR
- [x] Migrate codegen task structure: param reads, CLI parsing, probe init, thread launch from LIR
- [x] Migrate codegen firing emissions: actor calls, fork, probe, buffer I/O, fusion from LIR
- [x] Migrate codegen modal/ctrl: ctrl source read, mode dispatch, feedback resets from LIR
- [x] Add `codegen_from_lir()` public API
- [x] Fix LIR edge cases: probe parenthesization, buffer I/O retry naming, param type inference alignment, loop suppression for passthrough/block-transfer nodes, CLI param ordering
- [x] Switch pipeline driver (`main.rs`), snapshot tests, and bench to route through `codegen_from_lir`
- [x] Remove old inline-resolution code paths from codegen: `codegen.rs` 5,106 → 2,630 LOC (48.5% reduction); `codegen_from_lir()` signature narrowed from 9 to 4 params (`graph`, `schedule`, `options`, `lir`)

#### Phase 2c: Type Infer + Lower Migration ✅

- [x] Preserve type_args spans in `HirActorCall` (`Vec<String>` → `Vec<(String, Span)>`)
- [x] Reorder pipeline: `build_hir` before `type_infer` and `lower`
- [x] Migrate `type_infer` to consume `&HirProgram` — remove define body recursion (~70 LOC), add `target_call_id: CallId` to `WideningPoint`
- [x] Migrate `lower` to consume `&HirProgram` — remove `CallResolution::Define` filtering, match widenings by `CallId` instead of span
- [x] Fix CallId aliasing: fresh CallIds per define expansion (depth > 0)
- [x] Fix param type resolution for define-expanded calls (concrete_actors now includes expanded calls)
- [x] Update all callers (main, tests, bench) and snapshots
- [x] Regression tests: define polymorphism in two contexts, explicit type args, expanded calls in lower

### Phase 3: Pass Manager + Minimal Evaluation

- [x] Implement pass registry with declared inputs, outputs, invariants, and invalidation keys (`pass.rs`: `PassId`, `ArtifactId`, `PassDescriptor`, dependency resolution)
- [x] Compute minimal pass subset for each `--emit` target via `required_passes(terminal)` topological walk
- [x] Pipeline orchestration module (`pipeline.rs`): `CompilationState` with borrow-split artifacts, `run_pipeline()` with `on_pass_complete` callback
- [x] Migrate `main.rs` to delegate pass execution to `run_pipeline()` (parse + `--emit ast` remain outside runner)
- [x] Keep deterministic ordering and reproducible outputs (all 7 snapshots byte-identical)
- [x] Provenance type stubs (`Provenance` struct) for future cache-key use
- [ ] Implement deterministic `invalidation_key` hashing (deferred to Phase 3b)
- [ ] Add artifact hashing and reusable cache for heavy phases (deferred to Phase 3c)
- [ ] Integrate manifest/header provenance into cache keys and diagnostics (type stubs placed; implementation deferred to Phase 3b/3c)

### Phase 4: Verification Framework Generalization ✅

- [x] Generalize lower-only `Cert` model into stage-scoped verification framework (`StageCert` trait in `pass.rs`)
- [x] Add `verify_hir` (H1-H3), `verify_schedule` (S1-S2), `verify_lir` (R1-R2)
- [x] `verify_thir` not needed — ThirContext is a borrow-aggregation view; correctness validated transitively by upstream certs
- [x] Wire verification into pipeline runner with cert-failure-through-callback pattern
- [x] Promote proof obligations to CI gates (verification runs in `cargo test`; debug profile only — release matrix deferred to Phase 8)
- [x] Add regression corpus (`verify_regression.rs`: 7 example files + negative test)
- [x] Keep existing L1-L5 guarantees as strict subset (`impl StageCert for Cert`)

### Phase 5: Diagnostics Architecture Upgrade ✅

- [x] Introduce unified diagnostic payload: `code`, `level`, `message`, primary span, related spans, hint, cause chain (`diag.rs`)
- [x] Add stable diagnostic codes (54 codes: E0001-E0603, W0001-W0400) and compatibility policy (`DIAGNOSTIC_CODES.md`, uniqueness test)
- [x] Assign codes to all ~51 emission sites across 7 phase modules
- [x] Fix hint-dropping bug in `print_pipeline_diags()` (main.rs)
- [x] Add machine-readable diagnostics mode (`--diagnostic-format json`) with unified JSONL schema for both semantic and parse errors
- [x] Add exemplar `related_spans` and `cause_chain` for propagated constraint failures (E0200 L1 type consistency, E0303 type mismatch, E0304 SDF balance)
- [x] Migrate all diagnostic imports from `crate::resolve` to `crate::diag`
- [ ] Full provenance tracing through constraint solver (deferred to Phase 6+)
- [ ] Improve ambiguity and mismatch diagnostics with candidate and remediation suggestions (deferred to v0.5.x)

### Phase 6: Backend/Runtime Boundary Refactor

- [x] Move generic runtime shell logic (CLI parsing, probe init, duration wait, stats printing, thread launch policy) from generated C++ into `pipit_shell.h` runtime library
- [x] Make codegen emit compact descriptor tables (ParamDesc, TaskDesc, BufferStatsDesc, ProbeDesc) + `pipit::shell_main()` call
- [x] Keep runtime behavior compatibility for `--duration`, `--param`, `--probe`, `--probe-output`, `--stats`
- [x] Reduce generated C++ volume (~90-120 LOC saved per pipeline; `main()` from ~150 LOC to ~25 LOC descriptor tables)
- [x] Preserve deterministic generated symbol layout for test stability (7 snapshots updated, task function bodies unchanged)
- [x] ADR-026: Runtime Shell Library design decision documented
- [x] C++ unit tests for `shell_main()` (12 test cases)
- [x] E2E release regression tests (release codegen compiles with `-fsyntax-only`)

### Phase 7: Registry Determinism and Hermetic Build Inputs

#### Phase 7a (core) ✅

- [x] `--emit manifest`: scan headers → output canonical `actors.meta.json` (no `.pdl` required)
- [x] `--emit build-info`: source + registry provenance as JSON (source_hash, registry_fingerprint, manifest_schema_version, compiler_version)
- [x] Provenance stamping: generated C++ includes `// pcc provenance: source_hash=... registry_fingerprint=... version=...` comment header
- [x] Canonical fingerprint: SHA-256 of compact JSON (`canonical_json()`), decoupled from display formatting
- [x] Overlay/precedence rules documented and tested (`--actor-meta` = manifest-only; `-I` > `--actor-path`; `--emit manifest` + `--actor-meta` = usage error)
- [x] CI reproducibility tests (byte-identical outputs for same inputs; 6 tests)
- [x] ADR-027: Registry determinism and hermetic build inputs

#### Phase 7b (build integration) ✅

- [x] CMake integration: manifest generation/consumption in examples build graph (`PIPIT_USE_MANIFEST` option, default ON)
- [x] `add_custom_command` for `actors.meta.json` generation with explicit header inventory + scoped GLOB cross-check
- [x] `--actor-meta <generated_manifest>` consumption in `add_pdl_example()` targets
- [x] Dependency tracking: header change → manifest regeneration → recompile (`test_cmake_regen.sh` smoke test)
- [x] `build.sh` updated with `--no-manifest` flag and pinned PCC path
- [x] Legacy fallback path (`PIPIT_USE_MANIFEST=OFF`) with corrected DEPENDS (all headers listed)
- [x] Integration test: `manifest_then_compile_produces_valid_cpp`

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
- [x] `codegen.rs` footprint reduced by 48.5% from baseline (5,106 → 2,630 LOC) via LIR introduction — exceeds >=20% target
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
- **Pipeline (post Phase 2c)**: `parse → resolve → build_hir → type_infer(HIR) → lower(HIR) → graph(HIR) → ThirContext → analyze → schedule → LIR → codegen`
- **ADR numbering**: ADR-015 = spec alignment (from review/spec), ADR-016 = polymorphism & safe widening, ADR-017 = analysis-owned node port-rate resolution
- **v0.4.0 Phase 0 ADRs**: ADR-020 (pass manager/artifact model), ADR-021 (stable semantic IDs), ADR-022 (diagnostics model), ADR-023 (backward-compatibility gate)
- **v0.3.2** applies v0.3.0 polymorphism to 11 std actors; begins modular header split (`std_math.h`)
- **v0.5.x** now includes former v0.3.0 stdlib expansion backlog
- **pre-v0.4.0 open items** were moved to `v0.5.x` backlog (`Deferred Backlog from v0.3.x`)
- **v0.4.0** now tracks the compiler architecture rebuild (IR unification, pass manager, diagnostics/verification, backend/runtime boundary refactor)
- **v0.4.0 Phase 1** complete — snapshot safety net (7 insta tests), centralized graph/program helpers, stable semantic IDs (`CallId`/`DefId`/`TaskId`), span-key removal; all output byte-equivalent
- **New modules (Phase 1)**: `id.rs` (stable IDs + allocator), `program_query.rs` (shared set-directive helpers)
- **v0.4.0 Phase 2a** complete — HIR normalization (define expansion), ThirContext unified query wrapper, graph/analyze/schedule migrated off raw AST; all 500+ tests passing, byte-identical C++ output
- **New modules (Phase 2a)**: `hir.rs` (HIR types + AST→HIR builder with define expansion), `thir.rs` (ThirContext wrapper + precomputed metadata + dim-resolution queries)
- **ADR-024**: THIR-first IR unification strategy (HIR-first define expansion, ThirContext wrapper pattern, sub-phase ordering)
- **v0.4.0 Phase 2b** complete — LIR backend IR (`lir.rs`, ~2,050 LOC) pre-resolves all types/rates/dimensions/buffer metadata/actor params; codegen is now syntax-directed (reads LIR, no inference); `codegen.rs` reduced from 5,106 → 2,630 LOC (48.5%); `codegen_from_lir(graph, schedule, options, lir)` is the sole entry point; all 516 tests passing, byte-identical C++ output
- **New modules (Phase 2b)**: `lir.rs` (LIR types + builder: `build_lir(thir, graph, analysis, schedule) -> LirProgram`)
- **ADR-025**: LIR backend IR design (self-contained backend IR, structured data over pre-formatted strings, ThirContext-based builder)
- **v0.4.0 Phase 2c** complete — `type_infer` and `lower` migrated from raw AST to HIR; define body recursion eliminated (~70 LOC); widening matching upgraded from span-based to CallId-based; CallId aliasing fixed for define expansions; param type resolution bug fixed for define-expanded calls; 519 tests passing
- **v0.4.0 Phase 3** (partial) — pass registry (`pass.rs`: 9 PassIds, 11 ArtifactIds, dependency resolution), pipeline orchestration (`pipeline.rs`: borrow-split `CompilationState`, `run_pipeline()` with `on_pass_complete` callback), `main.rs` migrated to `run_pipeline()` (parse/`--emit ast` remain outside runner); `--emit graph-dot` now skips type_infer/lower; 526 tests passing, byte-identical C++ output. Invalidation hashing and caching deferred to Phase 3b/3c.
- **New modules (Phase 3)**: `pass.rs` (pass descriptors + dependency resolution), `pipeline.rs` (compilation state + pass orchestration + provenance stubs)
- **v0.4.0 Phase 6** complete — runtime shell library (`pipit_shell.h`): descriptor table + `shell_main()` replaces ~150 LOC inline shell per pipeline; codegen emits compact ParamDesc/TaskDesc/BufferStatsDesc/ProbeDesc arrays; preamble 13 includes → 3; 4 emit methods removed (~180 LOC net reduction in codegen.rs); `_probe_output_file` always generated; probe init gate is `probes.empty()` (no `#ifndef NDEBUG`); 12 C++ shell unit tests + 2 E2E release regression tests; 552 tests passing
- **New files (Phase 6)**: `runtime/libpipit/include/pipit_shell.h` (shell orchestration library), `runtime/tests/test_shell.cpp` (shell unit tests)
- **ADR-026**: Runtime Shell Library design (descriptor table approach, probe gate simplification, always-emit `_probe_output_file`)
- **v0.4.0 Phase 7a** complete — registry determinism and hermetic build inputs: `--emit manifest` (header scan → canonical JSON, no `.pdl` required), `--emit build-info` (SHA-256 provenance JSON), provenance comment stamped in generated C++ (`// pcc provenance: ...`), canonical fingerprint via `canonical_json()` (compact, decoupled from display formatting), overlay/precedence rules documented and tested, 6 reproducibility tests, 15 integration tests; Phase 7b (CMake integration) deferred
- **New methods (Phase 7a)**: `Registry::canonical_json()` (compact JSON for fingerprint), `compute_provenance()` (SHA-256 hashing), `Provenance::to_json()` (build-info output)
- **ADR-027**: Registry determinism and hermetic build inputs (manifest-first workflow, canonical fingerprint, output destination contract, overlay rules)
- **v0.4.0 Phase 7b** complete — CMake build integration: manifest-first workflow wired into `examples/CMakeLists.txt` (generate `actors.meta.json` once, all PDL targets consume via `--actor-meta`); `PIPIT_USE_MANIFEST` option (default ON) with legacy fallback; explicit `ALL_ACTOR_HEADERS` inventory with scoped GLOB cross-check (warns on unlisted headers, excludes `third_party/`); `build.sh` supports `--no-manifest` with pinned PCC path; `test_cmake_regen.sh` validates CMake dependency chain (header touch → manifest regen → C++ regen); integration test `manifest_then_compile_produces_valid_cpp`; 573 tests passing
- **v0.5.x** open items are currently deferred
- Performance characterization should inform optimization priorities (measure before optimizing)
- Spec files renamed to versioned names (`pipit-lang-spec-v0.3.0.md`, `pcc-spec-v0.3.0.md`); `v0.2.0` specs are frozen from tag `v0.2.2`
