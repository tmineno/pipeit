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
