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

**Goal**: Reduce compiler phase latency to the ~8000 ns/iter order with benchmark-locked refactors.

### Current Gate Status

| Gate | Target | Current | Status |
|---|---:|---:|---|
| build_lir/complex | ≤ 10,000 ns | **6,400** | **PASS** |
| emit_cpp/complex | ≤ 9,000 ns | **7,600** | **PASS** |
| analyze/complex | ≤ 8,500 ns | **5,800** | **PASS** |
| full_compile regression | no regression | ~41,000 | **PASS** |

<details>
<summary>M1: Measurement Hygiene — DONE</summary>

Label consistency, 3× median gate methodology, verification commands in report template, filename-sorted comparator. See `doc/performance/README.md`.

</details>

<details>
<summary>M2: Analyze Phase Optimization — DONE (5,800 ns, target ≤ 8,500)</summary>

- Merged `check_unresolved_frame_dims` + `check_dim_source_conflicts` → single `check_node_dim_constraints` (~450 ns algorithmic improvement)
- Cached `subgraphs_of()` results as `all_subgraphs` in `AnalyzeCtx` (~22 Vec allocs eliminated)
- Fixed benchmark scope: excluded `build_thir_context` from measured closure (9,200→5,800 ns measurement fix)
- Note: `node_actor_meta` HashMap precomputation tested and reverted (overhead for small graphs)

</details>

### M3: build_lir Stretch Goals (gate passed — incremental)

- [ ] Cache dim-resolution decisions per actor node in `resolve_missing_param_value`
- [ ] Memoize inferred wire type during subgraph edge-buffer construction
- [ ] Reduce `String`/`HashMap` churn in schedule-dim override construction

### M4: Compilation Parallelization (measurement-driven, deterministic output)

- [ ] `--compile-jobs N` with default `1`; keep single-thread baseline
- [ ] Benchmark matrix for parallel scaling (`N=1,2,4`) on `multitask`, `complex`, `modal`
- [ ] Parallelize per-task work: `analyze`, `schedule`, `build_lir`, `emit_cpp`
- [ ] Determinism guardrails: stable sort, deterministic diagnostics, byte-identical C++
- [ ] Auto-disable parallel path for tiny programs where overhead exceeds benefit

### M5: v0.4.5 Close

- [x] All 4 phase latency gates — **PASS**
- [x] Stable 3× median runs recorded
- [ ] Parallel compile speedup gate: requires M4

### M6: Runtime Benchmark Infrastructure

> `commit_characterize.sh` spends 90% of time (~40s) in C++ compilation via `run_all.sh`. Actual benchmark execution is ~2ms per binary.

- [x] Precompiled binaries: cache in `target/bench_cache/`, skip rebuild when source/headers unchanged
- [x] Parallel compilation: build all benchmark binaries concurrently before sequential execution

<details>
<summary>Completed optimizations (analyze, build_lir, emit_cpp, benchmarks)</summary>

**Benchmark decomposition**: Split `kpi/phase_latency/codegen` into `build_thir_context`, `build_lir`, `emit_cpp`. Legacy codegen kept for trend continuity.

**Analyze**: O(1) `HashSet` cycle guards, nested `span_derived_dims` HashMap, precomputed `node_port_rates` cache.

**build_lir**: Merged edge buffer/name construction, `EdgeAdjacency` + precomputed `firing_reps`, buffer reader metadata cache, benchmark scope fix.

**emit_cpp**: `task_index` HashMap for O(1) lookup, `strip_prefix` replaces `format!`, `indent_plus4()` pre-sized allocation, `Cow<str>` multi-input rewrite.

</details>

<details>
<summary>Verification commands</summary>

```sh
# Phase latency gates
./benches/compiler_bench_stable.sh \
  --filter 'kpi/phase_latency/(analyze|build_lir|emit_cpp)/complex' \
  --sample-size 40 --measurement-time 1.0

# Full compile regression check
./benches/compiler_bench_stable.sh \
  --filter 'kpi/full_compile_latency/(complex|modal)' \
  --sample-size 40 --measurement-time 1.0
```

</details>

---

## v0.5.x - Ecosystem & Quality of Life

### Deferred from v0.4.x: Compiler Latency Profiling & Recovery

- [ ] Phase benchmarks for `build_hir`, `type_infer`, `lower`, `build_thir`, `build_lir` + `--emit phase-timing`
- [ ] Formal KPI A/B benchmark against v0.3.4 baseline; record disposition in ADR-031
- [ ] Remove `LirInterTaskBuffer.skip_writes` and `.reader_tasks` (dead fields)
- [ ] Whole-program output cache (`cache.rs`): SHA-256 key, `$XDG_CACHE_HOME/pipit/v1/`, `--no-cache`

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

### Standard Actor Library Expansion

- [ ] **Phase 2**: `lpf`, `hpf`, `notch` filters; `ifft(N)`, `rfft(N)` transforms; `window(N, type)`
- [ ] **Phase 3**: WAV I/O, `iir`, `bpf`, resampling, `dct`, `hilbert`, `stft`/`istft`, `var`/`std`/`xcorr`, `gate`/`clipper`/`limiter`/`agc`
- [ ] **Infra**: per-actor unit tests, API reference, example pipelines, header split + `--actor-path`
- [ ] **Perf**: regression detection, CI flamegraphs, 24-hour drift test

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

- [ ] Migrate 54 `load_header()` call sites to golden manifest
- [ ] Rewrite registry.rs scanner-specific unit tests
- [ ] Delete dead functions: `load_header`, `scan_actors`, `strip_comments`, `parse_actor_macro`
- See review note: `agent-review/pipeit-clang/2026-02-28-text-scanner-removal-plan.md`

### Production Capabilities

- [ ] **Observability**: metrics (Prometheus/Grafana/OTel), built-in profiler, distributed tracing
- [ ] **Reliability**: fault tolerance, state checkpointing, graceful degradation
- [ ] **Security**: sandboxing, input validation, resource limits
- [ ] **Verification**: property-based testing, formal verification of scheduling, model checking

---

## Key References

- **Pipeline**: `parse → resolve → build_hir → type_infer → lower → graph → ThirContext → analyze → schedule → LIR → codegen`
- **ADRs**: 007 (shape), 009/010/014 (perf), 012 (KPI), 013 (PPKT), 016 (polymorphism), 020–023 (v0.4.0 arch), 028–030 (memory), 032–033 (PP manifest)
- **Spec is source of truth** over code; versioned specs frozen at tag points
- **Measure before optimizing** — performance characterization informs priorities
