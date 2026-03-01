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
| v0.4.3 | — | Bind-based external integration: `bind` grammar/IR/inference, stable IDs, `--emit interface`, `BindIoAdapter` codegen, runtime rebind |
| v0.4.4 | — | PP record manifest extraction (ADR-032), `--actor-meta` required (ADR-033, breaking), E0700 diagnostic, 667 tests |
| v0.4.5 | — | PSHM bind transport (`pipit_shm.h`, codegen lowering, SHM benchmark, cross-process example); phase latency optimization (all 4 gates PASS), analyze/build_lir/emit_cpp hot-path rewrites, benchmark infrastructure (build cache, parallel compile, quick mode), 667 tests |
| v0.4.6 | — | Bind infrastructure polish (`--emit interface`, `--interface-out`); compiler hotspot cleanup: precomputed canonical paths, pre-sorted adjacency, O(1) task lookup, hoisted-actor map, line-offset table; quantitative profiling protocol (`profile_ab.sh`, N=10 gate verification) |
| v0.4.7 | — | RingBuffer wait-loop: hybrid polling (spin→yield→sleep), time-based timeout (`set wait_timeout`), `WaitResult` enum (ADR-036), C++20 upgrade, 728 tests |
| v0.4.8 | — | Multi-channel spawn & shared buffer arrays: `shared` arrays, `spawn` clause, element/star refs (`name[idx]`/`name[*]`), gather/scatter codegen, E0026-E0035, 770 tests |

---

## v0.4.8.1 - Spawn Template Codegen Optimization

**Goal**: Reduce generated C++ code size for spawn-expanded tasks from O(N × body) to O(body + N). Replace N identical task function bodies with one parameterized function, ring buffer arrays, and loop-based gather/scatter.

**Motivation**: For large channel counts (e.g., 1024ch beamforming), the current approach clones the entire task function body N times. This causes compile-time and binary-size bloat proportional to channel count. The optimization emits a single parameterized function that accepts a channel index, with thin template wrappers for TaskDesc compatibility.

### M1: Spawn Family Metadata

- [ ] Add `SpawnFamily` struct to `spawn.rs`: family name, expanded task names, begin/end range, index var, referenced shared arrays
- [ ] Emit `Vec<SpawnFamily>` from `expand_spawns()` via `SpawnResult`
- [ ] Propagate through pipeline: `ResolvedProgram` → `LirProgram` (new field `spawn_families`)
- [ ] Unit tests: family metadata collection, const-ref bounds, multi-family programs

### M2: Ring Buffer & Stats Array Declarations

- [ ] `codegen.rs` `emit_shared_buffers()`: group element buffers by shared array family → emit `_ringbuf_NAME[CH]` array declaration instead of N individual `_ringbuf_NAME__i`
- [ ] Validate all family elements share same `cpp_type`, `capacity_tokens`, `reader_count`
- [ ] `codegen.rs` `emit_stats_storage()`: group spawn family stats → emit `_stats_NAME__spawn[CH]` array
- [ ] `codegen.rs` `emit_buffer_stats_descs()`: use array subscript for family buffers

### M3: Parameterized Spawn Function

- [ ] `codegen.rs` `emit_task_function()`: detect spawn family tasks → emit one `static void _spawn_NAME(int _ch)` using first instance's LIR body as template
- [ ] Ring buffer refs in spawn body: `_ringbuf_NAME__0` → `_ringbuf_NAME[_ch]`
- [ ] Stats refs: `_stats_NAME__spawn_0` → `_stats_NAME__spawn[_ch]`
- [ ] Edge buffer declarations: `thread_local` instead of bare `static` (each OS thread gets own copy; safe for future thread-pool scheduler)
- [ ] Emit `template<int _CH> void task_NAME__spawn() { _spawn_NAME(_CH); }` wrappers (decays to `void(*)()` for TaskDesc)
- [ ] Task table entries: `{"name[i]", task_NAME__spawn<i>, &_stats_NAME__spawn[i]}`
- [ ] Non-spawn tasks unchanged (backward compatible)

### M4: Loop-Based Gather/Scatter

- [ ] `codegen.rs` `emit_lir_gather_read()`: detect homogeneous family (all elements share tokens, reader_idx, reader_count) → emit `for` loop over `_ringbuf_NAME[_gi]` instead of N unrolled blocks
- [ ] `codegen.rs` `emit_lir_scatter_write()`: same loop optimization
- [ ] Fallback: keep unrolled codegen for heterogeneous families (should not occur with current semantics but defensive)

### M5: Tests & Close

- [ ] Add high-channel-count integration test (e.g., CH=16) verifying array + loop codegen pattern
- [ ] Verify existing 770+ tests pass unchanged
- [ ] Measure generated C++ line count: before vs after for `multichannel.pdl` at CH=4 and CH=16
- [ ] `cargo test && cargo clippy && cargo fmt --check`

### Design Notes

**Generated C++ before** (CH=4):

```cpp
static pipit::RingBuffer<float, 1024, 1> _ringbuf_raw__0;
static pipit::RingBuffer<float, 1024, 1> _ringbuf_raw__1;
static pipit::RingBuffer<float, 1024, 1> _ringbuf_raw__2;
static pipit::RingBuffer<float, 1024, 1> _ringbuf_raw__3;
static pipit::TaskStats _stats_capture__spawn_0;
// ... ×4

void task_capture__spawn_0() { /* 50 lines using _ringbuf_raw__0 */ }
void task_capture__spawn_1() { /* identical 50 lines using _ringbuf_raw__1 */ }
void task_capture__spawn_2() { /* identical 50 lines using _ringbuf_raw__2 */ }
void task_capture__spawn_3() { /* identical 50 lines using _ringbuf_raw__3 */ }
```

**Generated C++ after** (CH=4):

```cpp
static pipit::RingBuffer<float, 1024, 1> _ringbuf_raw[4];
static pipit::TaskStats _stats_capture__spawn[4];

static void _spawn_capture(int _ch) { /* 50 lines using _ringbuf_raw[_ch] */ }
template<int _CH> void task_capture__spawn() { _spawn_capture(_CH); }

// gather: loop instead of unrolled
for (int _gi = 0; _gi < 4; _gi++) {
    while (true) {
        if (_ringbuf_raw[_gi].read(0, _e + _gi * F, F)) break;
        // ... wait logic
    }
}
```

**Code growth**: O(body_size + N × 1 line) vs O(N × body_size).

**TaskDesc compatibility**: `template<int _CH> void task_NAME__spawn()` instantiates to `void(*)()` — no runtime API change needed.

**Edge buffer safety**: `thread_local` ensures each OS thread gets its own `static` edge buffers even when sharing a function body. No conflict with future thread-pool schedulers.

**Files to modify**: `spawn.rs`, `pipeline.rs`, `lir.rs`, `codegen.rs`, `codegen_compile.rs` (tests).

---

## v0.4.9 - Compiler Latency Stretch & Parallelization

**Goal**: build_lir stretch optimizations and multi-threaded compilation.

### M1: build_lir Stretch Goals (gate passed — incremental)

- [ ] Cache dim-resolution decisions per actor node in `resolve_missing_param_value`
- [ ] Memoize inferred wire type during subgraph edge-buffer construction
- [ ] Reduce `String`/`HashMap` churn in schedule-dim override construction

### M2: Compilation Parallelization (measurement-driven, deterministic output)

- [ ] `--compile-jobs N` with default `1`; keep single-thread baseline
- [ ] Benchmark matrix for parallel scaling (`N=1,2,4`) on `multitask`, `complex`, `modal`
- [ ] Parallelize per-task work: `analyze`, `schedule`, `build_lir`, `emit_cpp`
- [ ] Determinism guardrails: stable sort, deterministic diagnostics, byte-identical C++
- [ ] Auto-disable parallel path for tiny programs where overhead exceeds benefit

### M3: v0.4.9 Close

- [ ] Parallel compile speedup gate
- [ ] Stable 3× median runs recorded

---

## v0.5.x - Ecosystem & Quality of Life

### Deferred from v0.4.x: Compiler Latency Profiling & Recovery

- [ ] Phase benchmarks for `build_hir`, `type_infer`, `lower`, `build_thir`, `build_lir` + `--emit phase-timing`
- [ ] Formal KPI A/B benchmark against v0.3.4 baseline; record disposition in ADR-031
- [ ] Remove `LirInterTaskBuffer.skip_writes` and `.reader_tasks` (dead fields)
- [ ] Whole-program output cache (`cache.rs`): SHA-256 key, `$XDG_CACHE_HOME/pipit/v1/`, `--no-cache`
- [ ] `analyze`: reduce repeated all-subgraph traversal where checks can share one pass safely (deferred from v0.4.6: `mem::take` is zero-cost move, inter-pass dependencies prevent safe merging)
- [ ] `subgraph_index`: revisit small-graph indexing threshold (`INDEX_MIN_GRAPH_SIZE`) with measurement-backed default (deferred from v0.4.6: needs empirical data)

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

- **Pipeline**: `parse → spawn_expand → resolve → build_hir → type_infer → lower → graph → ThirContext → analyze → schedule → LIR → codegen`
- **ADRs**: 007 (shape), 009/010/014 (perf), 012 (KPI), 013 (PPKT), 016 (polymorphism), 020–023 (v0.4.0 arch), 028–030 (memory), 032–033 (PP manifest), 036 (ringbuf wait-loop)
- **Spec is source of truth** over code; versioned specs frozen at tag points
- **Measure before optimizing** — performance characterization informs priorities
