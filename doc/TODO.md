# Pipit Development Roadmap

## Completed Releases

| Version | Tag Date | Summary |
|---------|----------|---------|
| v0.1.0 | 2026-02-15 | Full pipeline, runtime library, 265 tests, basic benchmarks |
| v0.1.1 | — | Probe runtime wiring, release build guard, 8 e2e tests |
| v0.1.2 | — | 25 standard actors in `std_actors.h`, 143 tests, Doxygen docs |
| v0.2.0 | — | PortShape model (ADR-007), shape-aware rate resolution, SDF edge inference (§13.3.3), 353 tests |
| v0.2.1 | — | KPI benchmarks (ADR-012), scheduler/timer/ring-buffer optimization (ADR-009/010/014) |
| v0.2.2 | — | PPKT protocol (ADR-013), `socket_write`/`socket_read`, pipscope GUI, 6 waveform generators |
| v0.2.2a | — | Strict param types, modal switch semantics, ring buffer fairness, shared buffer optimization |
| v0.3.0 | — | Actor polymorphism (ADR-016), type inference, implicit widening, `lower.rs` L1–L5 proofs, manifest pipeline, 458 tests |
| v0.3.1 | — | Dimension inference fixes, shared-buffer block ops, actor construction hoisting, `dim_resolve.rs` |
| v0.3.2 | — | 11 polymorphic std actors, `std_math.h` split |
| v0.3.3 | — | Graph index layer, analyze worklist, codegen decomposition (~50% NLOC reduction in hotspots) |
| v0.3.4 | — | Measurement hardening, intra-task branch optimization; remaining hotspots deferred to v0.5.x |
| v0.4.0 | — | IR unification (AST→HIR→THIR→LIR), pass manager (ADR-020–023), diagnostics upgrade, `pipit_shell.h`, `codegen.rs` 5106→2630 LOC |
| v0.4.1 | — | MemoryKind enum (ADR-028), SPSC ring buffer (ADR-029), param sync simplification (ADR-030), `alignas(64)` edges |
| v0.4.2 | — | Diagnostics completion: all 10 E0100–E0206 enriched with `cause_chain`, `related_spans`, hints |
| v0.4.4 | — | PP record manifest extraction (ADR-032), `--actor-meta` required (ADR-033, breaking), E0700 diagnostic, 667 tests |

---

## v0.4.5 - Compiler Latency Refactoring

**Goal**: Reduce compiler phase latency (especially `analyze` + codegen path) to the ~8000 ns/iter order with benchmark-locked refactors.

> **Reference**: review note `agent-review/pipeit-refactor/2026-02-28-codegen-analyze-latency-strategy.md`

### Benchmark Decomposition (measurement first)

- [x] Split `kpi/phase_latency/codegen` into explicit buckets: `build_thir_context`, `build_lir`, `emit_cpp` (`codegen_from_lir` only)
- [x] Keep legacy `kpi/phase_latency/codegen` temporarily for trend continuity during migration
- [x] Add per-bucket `complex` scenario reporting to commit characterization notes

### Findings Snapshot (2026-02-28 decomposition run)

- [x] `build_lir` measured correctly (benchmark excluded THIR): **6,555 ns/iter** (target `<= 10,000` — **PASS**)
- [x] `emit_cpp` gate already passes: `7,633 ns/iter` (target `<= 9,000`)
- [ ] `analyze` gate not met yet: `9,791 ns/iter` (target `<= 8,500`; stable 3× median)
- [ ] Reconcile latest `full_compile` complex regression signal before declaring gate pass/fail (`tmp/bench_full_compile.txt`)

### Measurement & Report Hygiene (required for trustworthy tuning)

- [ ] Fix scenario label consistency (`complex` vs `multitask`) in benchmark summary tables
- [ ] Standardize gate decisions on stable 3× median runs (same CPU pinning + Criterion settings)
- [ ] Treat benchmark-definition changes (e.g., THIR excluded from `build_lir`) as separate from algorithmic speedups in reports
- [ ] Add one canonical verification command set to each performance report

### Analyze Refactoring & Optimization

- [x] Replace O(N) cycle guards (`Vec::contains`) in trace helpers with O(1) visited tracking
- [ ] Introduce reusable per-actor symbolic-dimension lookup plans (shared by span-derived and conflict checks)
- [ ] Merge repeated per-node passes (`record_span_derived_dims` + unresolved-dim checks + source-conflict checks) into one subgraph traversal
- [ ] Remove `subgraphs_of()` Vec allocation churn in analyze hot paths (use non-alloc traversal helper)
- [ ] Cache per-actor dim metadata (symbol list / param index / shape index) and reuse across all nodes
- [x] Reduce temporary allocation churn in shape conflict checks (nested `span_derived_dims` eliminates `sym.clone()`; removed `.cloned()` in conflict checks)
- [x] Eliminate redundant end-of-pass graph walks where data can be collected during existing traversals (precomputed `node_port_rates`)

### LIR/Codegen Refactoring & Optimization

#### `build_lir` priority actions (dominant bottleneck)

- [x] Eliminate duplicated edge buffer/name construction passes; build one subgraph edge context and reuse (`build_edge_buffers_and_names`)
- [x] Cache per-subgraph incoming/outgoing edge adjacency and node repetition lookups (`EdgeAdjacency` struct, precomputed `firing_reps` HashMap)
- [x] Reduce string/HashMap key churn in dim override resolution (nested `span_derived_dims`, eliminated `.to_string()` in lookups)
- [x] Precompute shared-buffer reader metadata once per buffer (`buffer_readers` cache in `LirBuilder`)
- [x] Fix `build_lir` benchmark to exclude THIR rebuild from measured closure (was measuring THIR+LIR)
- [ ] Cache dim-resolution decisions per actor node to avoid repeated shape/span/schedule lookups in `resolve_missing_param_value` and schedule overrides
- [ ] Memoize inferred wire type during subgraph edge-buffer construction to avoid repeated trace walks
- [ ] Reduce transient `String`/`HashMap` churn in schedule-dim override construction for empty/single-symbol cases

#### `emit_cpp` follow-up (already below gate, keep improving)

- [x] Precompute hoisted actor lookup maps; remove repeated `format!` + linear `.find()` lookups in firing loops (`task_index` HashMap, `strip_prefix` in fused chain)
- [x] Reduce repeated indent/call-expression string construction in hot emission paths (`indent_plus4()`)
- [x] Reduce multi-input temporary allocation churn in `build_lir_input_ptr` and related emit helpers (inline iteration, `Cow<str>`)
- [x] Improve `cpp_source` output buffer sizing heuristic (dynamic `2048 + tasks * 200`)

### Compilation Parallelization (measurement-driven, deterministic output)

- [ ] Add `--compile-jobs N` (or env equivalent) with default `1`; keep single-thread path as baseline/reference
- [ ] Add benchmark matrix for compile parallel scaling (`N=1,2,4`) on `multitask`, `complex`, `modal`
- [ ] Parallelize per-task/subgraph work where dependencies are independent:
- [ ] `analyze`: run task-local checks/inference in parallel, then deterministic merge of diagnostics/results
- [ ] `schedule`: parallelize per-task schedule construction with stable reduction order
- [ ] `build_lir`: parallelize per-task LIR construction, then stable task ordering in final IR
- [ ] `emit_cpp`: parallelize task-level code emission, then deterministic concatenation
- [ ] Enforce determinism guardrails: stable sort before merge, deterministic diagnostic order, byte-identical generated C++ across repeated runs
- [ ] Avoid lock-heavy shared mutation in hot paths (prefer thread-local accumulation + final reduce)
- [ ] Add compatibility fallback: auto-disable parallel path for tiny programs where overhead exceeds benefit

### Acceptance Gates (must pass before close)

- [x] `kpi/phase_latency/build_lir/complex <= 10k ns/iter` (current: **6,555** — **PASS**, 3× median stable)
- [ ] `kpi/phase_latency/analyze/complex <= 8.5k ns/iter` (current: **9,791** — MISS, deferred; `node_actor_meta` precomputation tested+reverted)
- [x] `kpi/phase_latency/emit_cpp/complex <= 9.0k ns/iter` (current: **7,633** — PASS)
- [ ] `kpi/full_compile_latency/{complex,modal}` no regression (reconfirm after scenario-label cleanup + stable 3× median rerun)
- [x] Stable 3× median runs recorded in `tmp/build-lir-benchmark-fix/report.md`
- [ ] Parallel compile speedup gate (opt-in `--compile-jobs`): `multitask`/`modal` full-compile latency improves vs `jobs=1` with no correctness/determinism regressions

### Verification Commands (v0.4.5 performance work)

- [ ] `./benches/compiler_bench_stable.sh --filter 'kpi/phase_latency/(analyze|build_lir|emit_cpp)/complex' --sample-size 40 --measurement-time 1.0 --warm-up-time 0.2`
- [ ] `./benches/compiler_bench_stable.sh --filter 'kpi/full_compile_latency/(complex|modal)' --sample-size 40 --measurement-time 1.0 --warm-up-time 0.2`
- [ ] `for n in 1 2 4; do PIPIT_COMPILE_JOBS=$n ./benches/compiler_bench_stable.sh --filter 'kpi/full_compile_latency/(multitask|modal)' --sample-size 30 --measurement-time 0.8 --warm-up-time 0.2; done`

---

## v0.5.x - Ecosystem & Quality of Life

**Goal**: Make Pipit easier to use and deploy in real projects.

### Deferred from v0.4.x: Compiler Latency Profiling & Recovery

> **Reference**: review-0004. Acceptance gate: cold-compile KPI within 10% of v0.3.4 baseline (`7248b44`).

- [ ] Phase benchmarks for `build_hir`, `type_infer`, `lower`, `build_thir`, `build_lir` + `--emit phase-timing`
- [ ] Explicit timing for `build_thir_context()` (currently untimed)
- [ ] Formal KPI A/B benchmark against v0.3.4 baseline; record disposition in ADR-031
- [ ] Remove `LirInterTaskBuffer.skip_writes` and `.reader_tasks` (dead fields)
- [ ] Whole-program output cache (`cache.rs`): SHA-256 key, `$XDG_CACHE_HOME/pipit/v1/`, skip-cache-if-warnings, `--no-cache`
- [ ] Deterministic `invalidation_key` hashing (deferred from v0.4.1)

### Deferred Backlog from v0.3.x–v0.4.x

- [ ] Narrowing conversion warnings (v0.3.0, SHOULD-level, §3.4)
- [ ] Comprehensive golden test suite — full type matrix (v0.3.0)
- [ ] Diagnostic polish — multi-line error context, candidate suggestions (v0.3.0)
- [ ] Socket-loopback benchmark (v0.3.1, port-bind infra issue)
- [ ] `codegen.rs` `param_cpp_type` / literal helpers optimization (v0.3.4)
- [ ] `analyze.rs` `record_span_derived_dims` optimization (v0.3.4)
- [ ] `ActorMeta` clone reduction in type_infer hot paths (v0.3.4)
- [ ] String/HashMap churn reduction in monomorphization keys (v0.3.4)
- [ ] Cache PP extraction outputs by header content hash (v0.4.4)
- [ ] Skip manifest regen when actor-signature set unchanged (v0.4.4)
- [ ] Re-benchmark two-step manifest workflow (v0.4.4)
- [ ] KPI exit criteria: complex/modal ≥5% improvement vs v0.3.3, no regressions (v0.3.4)
- [ ] Task-internal branch parallelization study — safety gate, effect classification, prototype (v0.3.4)

### Standard Actor Library Expansion

#### Phase 2: Signal Processing Basics

- [ ] Simple filters: `lpf`, `hpf`, `notch` (Butterworth/biquad)
- [ ] Transforms: `ifft(N)`, `rfft(N)` (validate against FFTW)
- [ ] Windowing: `window(N, type)` — hann, hamming, blackman

#### Phase 3: Advanced Signal Processing

- [ ] WAV file I/O: `wavread(path)`, `wavwrite(path)` (16/24/32-bit PCM)
- [ ] Advanced filters: `iir(b, a)`, `bpf(low, high, order)`
- [ ] Resampling: `resample(M, N)`, `interp(N)`, `downsample(N)`
- [ ] Advanced transforms: `dct(N)`, `hilbert(N)`, `stft(N, hop)`, `istft(N, hop)`
- [ ] Advanced statistics: `var`, `std`, `xcorr`, `acorr`, `convolve`
- [ ] Control flow: `gate`, `clipper`, `limiter`, `agc`

#### Infrastructure

- [ ] Per-actor unit test framework + edge case testing (zero, infinity, NaN)
- [ ] Actor API reference, usage examples, performance docs
- [ ] Example pipelines: audio effects, SDR, sensor processing
- [ ] Header split: `io.h`, `filters.h`, etc. + `--actor-path` discovery

#### Performance & Benchmarking

- [ ] Regression detection with statistical comparison, CI integration, flamegraphs
- [ ] Performance tuning guide (CPU affinity, NUMA, compiler flags)
- [ ] Extended testing: 24-hour drift test, comparison with GNU Radio

### Runtime & Build

- [ ] Round-robin scheduler with thread pools
- [ ] Platform support (macOS, Windows native)
- [ ] LSP server for IDE integration
- [ ] CMake integration, install target, pkg-config, package manager

---

## v0.5.0 - Advanced Features (Future)

- [ ] **Compiler optimizations**: fusion, constant propagation, dead code elimination, actor inlining
- [ ] **Real-time scheduling**: priority-based, deadline guarantees, CPU affinity, NUMA
- [ ] **Heterogeneous execution**: GPU (CUDA/OpenCL), FPGA codegen, accelerator offload
- [ ] **Distributed computing**: cross-node pipelines, network-transparent buffers, fault tolerance

---

## v0.6.0 - Production Hardening (Future)

### Legacy Text Scanner Removal (deferred from v0.4.4)

- [ ] Migrate 54 `load_header()` call sites (17 files) to golden manifest
- [ ] Rewrite registry.rs scanner-specific unit tests
- [ ] Delete dead functions: `load_header`, `scan_actors`, `strip_comments`, `parse_actor_macro`
- [ ] Mark as breaking API change (`refactor!:`)
- See review note: `agent-review/pipeit-clang/2026-02-28-text-scanner-removal-plan.md`

### Production Capabilities

- [ ] **Observability**: metrics (Prometheus/Grafana/OTel), built-in profiler, distributed tracing
- [ ] **Reliability**: fault tolerance, state checkpointing, graceful degradation
- [ ] **Security**: sandboxing, input validation, resource limits
- [ ] **Verification**: property-based testing, formal verification of scheduling, model checking

---

## Key References

- **Pipeline**: `parse → resolve → build_hir → type_infer → lower → graph → ThirContext → analyze → schedule → LIR → codegen`
- **ADRs**: 007 (shape inference), 009/010/014 (perf), 012 (KPI), 013 (PPKT), 015 (spec alignment), 016 (polymorphism), 017 (port-rate), 020–023 (v0.4.0 arch), 028–030 (memory), 032–033 (PP manifest)
- **Spec is source of truth** over code; versioned specs frozen at tag points
- **Measure before optimizing** — performance characterization informs priorities
