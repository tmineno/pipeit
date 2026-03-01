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
| v0.4.6 | — | Bind infrastructure polish: interface manifest opt-in (`--emit interface`, `--interface-out`) |

---

## v0.4.7 - RingBuffer Wait-Loop & Timeout Policy

**Goal**: Replace busy-retry wait loops in inter-task ringbuf edges with blocking wait primitives and time-based timeouts. See review note: `agent-review/pipeit-refactor/2026-03-01-ringbuf-wait-loop-scheduler-review.md`.

### M1: Mechanical — Wait-Policy Plumbing (no behavior change)

- [ ] Add `WaitResult` enum (`ready | timeout | stopped`) in `pipit.h`
- [ ] Add `wait_readable(reader_idx, tokens, stop, timeout)` and `wait_writable(tokens, stop, timeout)` stubs in `RingBuffer` (return `ready` immediately, no-op)
- [ ] Add wait-policy config types in codegen/THIR (plumbing only, not wired)
- [ ] ADR for wait-loop policy contract (Option C rationale, fallback strategy, timeout semantics)

### M2: Behavior Change — Atomic Wait/Notify + Fallback

- [ ] Implement `atomic_wait`/`atomic_notify` path in `RingBuffer::wait_readable` / `wait_writable` (C++20)
- [ ] Implement hybrid-polling fallback path (`spin → yield → sleep`) when `atomic_wait` unavailable
- [ ] Switch codegen `emit_lir_buffer_read` / `emit_lir_buffer_write` to emit wait-enabled loop shape
- [ ] Replace attempt-based timeout (1,000,000 retries) with time-based timeout (default 50 ms)
- [ ] Add runtime tests: empty/full transitions, stop signaling, timeout, concurrent producer/consumer stress

### M3: Optimization — Tuning & Benchmarks

- [ ] Benchmark wait-enabled vs old retry-yield loops (ringbuf contention, timer jitter, deadline miss)
- [ ] Tune hybrid spin/yield/sleep thresholds based on benchmark data
- [ ] Record benchmark results in `doc/performance/`

### M4: v0.4.7 Close

- [ ] All compiler tests pass (`cargo test`)
- [ ] All runtime tests pass (including new wait-loop tests)
- [ ] Ringbuf contention benchmark shows improvement over v0.4.5 baseline

---

## v0.4.8 - Multi-Channel Spawn & Shared Buffer Array

**Goal**: Implement `shared` buffer arrays (family), spawn clause for static task replication, element/full-array references (`name[idx]` / `name[*]`), and gather/scatter semantics. See lang spec §5.3.1, §5.4.5, §5.7, §11.6, §13.2.3.

### M1: Parse & AST (mechanical — no behavior change)

- [ ] Lexer: add `shared` keyword, `*` (star) token, `..` (range dots) token
- [ ] Parser: `shared_stmt` → `'shared' IDENT '[' shape_dim ']'`
- [ ] Parser: `spawn_clause` → `'[' IDENT '=' range_expr ']'` on `task_stmt`
- [ ] Parser: `buffer_ref` → `IDENT` / `IDENT '[' index_expr ']'` / `IDENT '[' '*' ']'` in `pipe_source` and `sink`
- [ ] AST node types: `SharedDecl`, `SpawnClause`, `BufferRef(name, index)` with `BufferIndex::None | Literal(u32) | Ident(String) | Star`
- [ ] Unit tests: parse round-trip for `shared`, spawn, element ref, star ref

### M2: Spawn Expansion (new compiler pass — before name resolve)

- [ ] Implement spawn expansion pass: expand `clock name[idx=begin..end]` into N independent `clock` tasks (`name[0]` … `name[N-1]`)
- [ ] Substitute spawn index variable in actor arguments and buffer subscripts within each expanded task body
- [ ] Validate spawn range: `begin < end`, both positive compile-time integers; emit diagnostic on violation
- [ ] Insert pass into pipeline between parse and resolve (spec §8: "spawn 展開は name resolve / 型推論 / SDF 解析の前に実行")
- [ ] Unit tests: expansion output, index substitution, range validation errors

### M3: Shared Buffer Array — Name Resolution & Validation

- [ ] Register `shared` declarations in resolve scope; resolve `name[idx]` to individual buffer elements
- [ ] Resolve `name[*]` to gather/scatter virtual port referencing all family elements
- [ ] Compile-time index range check: `0 <= idx < N`, emit diagnostic for out-of-bounds
- [ ] Extend single-writer constraint to family elements; reject `-> name[*]` + `-> name[idx]` conflicts
- [ ] Unit tests: resolution, index range errors, writer conflict errors

### M4: SDF Graph & Analysis — Shape Lift & Family Constraints

- [ ] SDF graph construction: `name[idx]` as independent shared-buffer edge; `name[*]` as gather/scatter virtual node
- [ ] Shape lift (§13.2.3): `name[*]` → 2D shape `[CH, F]` (channel dim × frame dim)
- [ ] Family contract validation: all elements of `name[*]` must share same dtype and frame size `F`
- [ ] Rate constraints for `name[*]`: gather requires uniform `Cr_elem × fr`; scatter requires divisible total rate
- [ ] Bind direction inference: extend to `-> name[*]` (out-bind) and `@name[*]` (in-bind)
- [ ] Diagnostics: E-codes for spawn range error, index out-of-bounds, family contract mismatch (spec §7)
- [ ] Unit tests: shape inference, contract errors, bind direction with families

### M5: Schedule & Codegen

- [ ] Schedule generation for spawned tasks (each expanded task scheduled independently)
- [ ] LIR: buffer array element mapping — each `name[idx]` lowers to a distinct `LirInterTaskBuffer`
- [ ] Codegen: `@name[idx]` / `-> name[idx]` emit same C++ as plain shared buffers (element-wise)
- [ ] Codegen: `@name[*]` (gather) — emit sequential reads from `name[0]..name[N-1]` into contiguous frame
- [ ] Codegen: `-> name[*]` (scatter) — emit slice-and-write from contiguous frame to each element
- [ ] Integration tests: compile §11.6 example (`codegen_compile.rs`)
- [ ] Runtime tests: multi-channel spawn end-to-end (`runtime_actors.rs`)

### M6: v0.4.8 Close

- [ ] All compiler tests pass (`cargo test`)
- [ ] §11.6 example compiles and runs correctly
- [ ] Spawn expansion + gather/scatter codegen verified on 24-channel example

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
