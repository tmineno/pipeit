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

## v0.4.0 - Compiler Architecture Rebuild (IR Unification + Pass Manager) ✅

**Goal**: Perform a large architecture transition so all downstream phases consume one typed/lowered IR contract, pass execution is dependency-driven, and backend/runtime responsibilities are clearly separated.

- [x] Contract freeze completed via `pcc-spec-v0.4.0.md` and ADR-020/021/022/023, including backward-compatibility gate policy.
- [x] Production pipeline unified to `AST -> HIR -> THIR -> LIR`, with graph/analyze/schedule/codegen consuming the new contracts.
- [x] Pass manager shipped with minimal-pass evaluation (`required_passes`) and deterministic orchestration.
- [x] Stage-scoped verification framework integrated into compiler flow and CI.
- [x] Diagnostics architecture upgraded (stable codes, JSON diagnostics mode, related spans, cause chain).
- [x] Backend/runtime boundary refactored through runtime shell extraction (`pipit_shell.h`) and descriptor-based codegen.
- [x] Registry determinism delivered (`--emit manifest`, `--emit build-info`, provenance stamping, manifest-first CMake integration).
- [x] Migration/test hardening completed (HIR/THIR/LIR snapshots, pipeline equivalence tests, property tests, full matrix green).
- [x] Major codegen simplification achieved (`codegen.rs`: 5,106 -> 2,630 LOC).

### Deferred Follow-up

- Pre-refactor baseline for KPI comparison: `7248b44` (v0.3.4).
- Post-Phase-2c observed regression point: `e758c03` (simple +14.7%, multitask +31.9%, complex +36.8%, modal +47.7%).
- Open items are deferred to `v0.4.x` and grouped below by patch-version complexity.

---

## v0.4.1 - Memory Plan: Low-Copy Codegen & SPSC Optimization ✅

**Goal**: Classify edge memory kinds in LIR, specialize SPSC ring buffers, and reduce param sync overhead.

> **Reference**: review-0003 (memory plan), ADR-028/029/030. Audited for over-engineering: items where modern compilers (GCC 13+, Clang 17+) already optimize equivalently were deferred.

- [x] Add `MemoryKind` enum (`Local`, `Shared`, `Alias`) to `LirEdgeBuffer` and `LirInterTaskBuffer` (ADR-028)
- [x] Add `alignas(64)` to intra-task non-feedback edge buffers
- [x] SPSC `RingBuffer<T, Capacity, 1>` partial specialization with API-compatible overloads (ADR-029)
- [x] Add `reader_count` to `LirBufferIo`; SPSC comment in codegen retry loops
- [x] Simplify param sync to single acquire load, remove `_param_read` (ADR-030)
- [x] Add `--experimental` CLI flag plumbing (reserved for Phase C gating)
- [x] New `test_ringbuf.cpp`: 9 SPSC tests (correctness, wraparound, concurrent stress, multi-reader coexistence)

### Deferred to v0.5.x

- Scalarization (token=1 edges) — only 7 edges; compilers optimize `float[1]` identically to `float`
- `assume_aligned` / `__restrict__` — `assume_aligned` redundant for `alignas` static; `__restrict__` needs measurement
- Locality scoring for scheduler — ready-set typically 1-3 nodes; <2% expected benefit
- `_in_*` concatenation copy reduction — requires LIR `offset`/`stride` metadata extension
- SPSC retry tuning (spin/yield/backoff) — measure before tuning
- SPSC relaxed memory ordering evaluation — needs proof, measure before changing
- Phase C: Block pool & pointer ring — gated behind `--experimental`, high complexity
- Deterministic `invalidation_key` hashing — no caching infrastructure to consume keys

## v0.4.2 - Diagnostics Completion ✅

**Goal**: Complete diagnostics provenance and ambiguity guidance to improve debuggability and remediation clarity.

- [x] Enrich `type_infer.rs` E0100/E0101/E0102 with `cause_chain`, `related_spans`, and actionable hints
- [x] Refactor `infer_type_from_args()` to return partial results (`Option<Vec<Option<PipitType>>>`) for E0102 provenance
- [x] Enrich `lower.rs` E0200-E0206 with `cause_chain`, `related_spans`, and remediation hints
- [x] Dedicated tests for E0202 (L3), E0204 (L4), and E0100 paths
- [x] All 10 diagnostics in type_infer + lower now have full provenance (cause_chain + hints)

## v0.4.4 - PP Record Manifest Extraction & Manifest-Required Inputs (Breaking) ✅

**Goal**: Replace text-based actor metadata scanning with preprocessor-based extraction (ADR-032) and require `--actor-meta` for all compilation stages (ADR-033, breaking change).

- [x] **M0: Contract lock (spec/ADR/docs)**
  - [x] ADR-032: PP record manifest extraction approach
  - [x] ADR-033: Manifest-required inputs (breaking change)
  - [x] Update `pcc-spec-v0.4.0.md` (§5.2, §5.3, §8, §10.5, §10.6.8)
  - [x] Update usage guide, README, v0.4.4 release note
- [x] **M1: PP record extraction (replaces text scanner)**
  - [x] `scan_actors_pp()`: build probe TU, invoke preprocessor, parse `PIPIT_REC_V1(...)` records
  - [x] Probe TU redefines `ACTOR` macro → structured records, pipes to `clang++ -E -P -x c++ -std=c++20 -`
  - [x] Byte-identical manifest output verified against golden fixture
  - [x] `PreprocessorError` variant for compiler/preprocessing failures (exit code 3)
- [x] **M3: Manifest-required enforcement (breaking)**
  - [x] E0700 diagnostic for missing `--actor-meta` on compile stages (exit code 2)
  - [x] `emit_usage_error()` centralized helper respecting `--diagnostic-format`
  - [x] `--emit ast` and `--emit manifest` unaffected (no `--actor-meta` needed)
  - [x] All 5 test files updated with `shared_manifest()` helpers (OnceLock)
  - [x] E0700 tests: human format, JSON format, stage-aware messages
- [x] **M4: Build integration + cleanup**
  - [x] CMakeLists.txt: removed legacy `PIPIT_USE_MANIFEST=OFF` path
  - [x] `build.sh`: removed `--no-manifest` option
  - [x] 16 PP extraction unit tests (parse, split, unescape, invoke, failure paths)
  - [x] 667 total tests passing

---

## v0.5.x - Ecosystem & Quality of Life

**Goal**: Make Pipit easier to use and deploy in real projects.

> **Status**: Deferred. All unchecked (`- [ ]`) items in this `v0.5.x` section are deferred.

### Deferred from v0.4.x: Compiler Latency Profiling & Recovery

> **Reference**: review-0004 (deferred v0.4.3 plan). Acceptance gate: cold-compile KPI within 10% of v0.3.4 baseline (`7248b44`). Warm-cache CLI latency is a separate informational metric.

- [ ] **Measurement infrastructure**
  - [ ] Add phase benchmarks for build_hir, type_infer, lower, build_thir, build_lir to `kpi/phase_latency`
  - [ ] Add `--emit phase-timing` for machine-readable JSON output (store timings in `CompilationState`)
  - [ ] Add explicit timing for `build_thir_context()` (currently untimed, hidden hotspot)
- [ ] **Baseline & disposition**
  - [ ] Run formal KPI A/B benchmark against v0.3.4 baseline (`compiler_bench_stable.sh --baseline-ref v0.3.4`)
  - [ ] Record release disposition for compile-latency regression in ADR-031
- [ ] **LIR dead field removal**
  - [ ] Remove `LirInterTaskBuffer.skip_writes` and `.reader_tasks` (computed but never read by codegen)
- [ ] **Whole-program output cache** (`cache.rs`)
  - [ ] Cache key: SHA-256 of source_hash + registry_fingerprint + compiler_version + codegen_options + include_paths
  - [ ] File-based cache at `$XDG_CACHE_HOME/pipit/v1/`, atomic write, best-effort I/O
  - [ ] Skip-cache-if-warnings policy (only cache warning-free compilations)
  - [ ] `--no-cache` CLI flag
- [ ] **Not actionable (investigated, no change needed)**
  - [x] `precompute_metadata()` duplication audit — no such function; analysis→LIR data flow is reasonable
  - [x] Lazy LIR field materialization — codegen uses all fields
- [ ] **Deterministic `invalidation_key` hashing** (deferred from v0.4.1 — no caching infrastructure to consume keys)

### Deferred Backlog from v0.3.x

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

- [ ] **From v0.4.4 / Priority 3: Manifest generation + refresh costs**
  - [ ] Cache PP extraction outputs keyed by header content hashes
  - [ ] Skip manifest regeneration when actor-signature set is unchanged
  - [ ] Re-benchmark two-step workflow (`manifest -> build-info/cpp`) for `simple`, `multitask`, `modal`

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

### Legacy Text Scanner Removal (deferred from v0.4.4)

- [ ] Migrate 54 test/bench `load_header()` call sites across 17 files to golden manifest (`Registry::load_manifest`)
- [ ] Migrate `test_registry_with_extra_header()` and `dimension_param_order_warning` to `Registry::insert()` pattern
- [ ] Rewrite registry.rs scanner-specific unit tests (delete dead-code tests, rewrite registry-behavior tests)
- [ ] Delete dead functions: `load_header`, `scan_actors`, `strip_comments`, `parse_actor_macro`
- [ ] Mark `load_header` removal as breaking API change (`refactor!:`)
- See review note: `agent-review/pipeit-clang/2026-02-28-text-scanner-removal-plan.md`

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
- **v0.4.0 summary**: architecture rebuild completed across contract freeze, IR unification, pass-manager orchestration, verification/diagnostics upgrade, runtime-shell extraction, registry determinism, and migration hardening.
- **v0.4.0 delivered artifacts**: HIR/THIR/LIR production pipeline, `codegen_from_lir` path, unified diagnostics with stable codes in `pcc-spec-v0.4.0.md` §10.4-§10.6, `--emit manifest` / `--emit build-info`, and manifest-first CMake integration.
- **v0.4.x deferred work placement**: follow-up items from v0.4.0 were grouped into `v0.4.1`/`v0.4.2`/`v0.4.3` by complexity and release criticality. v0.4.3 (latency profiling & recovery) was deferred to v0.5.x after planning review (see review-0004).
- **v0.4.1 summary**: MemoryKind classification (ADR-028), SPSC ring buffer (ADR-029), param sync simplification (ADR-030), `alignas(64)` edge buffers, `--experimental` flag. Audited for over-engineering; scalarization/assume_aligned/locality-scoring deferred.
- **v0.4.1 ADRs**: ADR-028 (edge memory classification), ADR-029 (SPSC ring buffer specialization), ADR-030 (param sync simplification)
- **v0.4.2 summary**: Diagnostics completion — enriched all 10 diagnostics in `type_infer.rs` (E0100-E0102) and `lower.rs` (E0200-E0206) with `cause_chain`, `related_spans`, and actionable hints. Refactored `infer_type_from_args()` for partial-result provenance.
- **v0.4.3 deferred**: Compiler latency profiling & recovery planned but deferred to v0.5.x. Full plan in review-0004. Key findings: ThirContext untimed, LIR has 2 dead fields, whole-program output cache designed with skip-cache-if-warnings policy and include_paths in cache key.
- **v0.4.4 summary**: PP record manifest extraction (ADR-032) replaces text scanner (~440 lines deleted). Breaking change: `--actor-meta` required for `--emit cpp|exe|build-info|graph|graph-dot|schedule|timing-chart` (ADR-033). E0700 diagnostic, centralized usage-error emitter, 16 PP extraction unit tests. 667 total tests passing.
- **v0.4.4 ADRs**: ADR-032 (PP record manifest extraction), ADR-033 (manifest-required inputs)
- **v0.5.x** open items are currently deferred; now also includes former v0.4.3 latency work
- **v0.4.4 deferred**: Legacy text scanner removal (54 test call sites across 17 files) deferred to v0.6.x — full migration plan in review note `2026-02-28-text-scanner-removal-plan.md`
- Performance characterization should inform optimization priorities (measure before optimizing)
- Spec files renamed to versioned names (`pipit-lang-spec-v0.3.0.md`, `pcc-spec-v0.3.0.md`); `v0.2.0` specs are frozen from tag `v0.2.2`
