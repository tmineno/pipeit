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
- Open items are deferred to `v0.4.x` and grouped below by priority (criticality → performance impact → complexity).

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

### Deferred to v0.4.4

- Deterministic `invalidation_key` hashing — no caching infrastructure to consume keys

---

## v0.4.x Priority Overview (Remaining)

| Version | Focus | Criticality | Perf Impact | Complexity | Depends on |
|---------|-------|-------------|-------------|------------|------------|
| v0.4.1 ✅ | Memory plan (codegen + SPSC) | Medium | Very High | Medium-High | — |
| v0.4.2 ✅ | Bind-based external integration | **Critical** | Indirect | Medium-High | v0.4.0 |
| v0.4.3 | PSHM shared-memory transport | **Critical** | Very High | High | v0.4.2 P1-P3 |
| v0.4.4 | Compiler latency + deterministic caching | Medium | Medium | Medium | — |
| v0.4.5 | Diagnostics completion | Low | — | Medium | — |

**Scheduling note**: v0.4.3 Phase 2 (PSHM runtime core) can be developed in parallel with v0.4.2 later phases since it is runtime-only work with no compiler dependency.

```text
Time →
v0.4.2 (Bind P1-P3)    ████████████████
v0.4.3 (PSHM P2)              ░░░░░░░░░░  (parallel: runtime-only, no compiler dep)
v0.4.2 (Bind P4-P6)                    ████████
v0.4.3 (PSHM P3-P6)                    ████████████  (after bind P1-P3)
v0.4.4 (Latency)                        ░░░░░░░░░░░░░░░░  (anytime)
v0.4.5 (Diagnostics)                                    ████
```

---

## v0.4.2 - Bind-Based External Integration ✅

**Goal**: Implement `bind`-first external integration with compiler-generated stable IDs and iteration-boundary-safe runtime rebind.

> **Priority rationale**: Critical foundation — unlocks PSHM transport (v0.4.3) and all future transport backends. No external integration is possible without this infrastructure.

### Phase 1: Frontend & IR Wiring (Mechanical) ✅

- [x] Add `bind` to lexer/parser/AST (`bind <name> = <endpoint>`) with source-span diagnostics
- [x] Thread bind declarations into HIR/THIR/LIR as first-class artifacts (no ad-hoc side tables)
- [x] Add structured bind args through all IR layers (`LirBindValue` variants: String, Int, Float, Size, Freq, Ident)
- [x] Add diagnostics E0024 (duplicate bind), E0025 (reserved: bind target not referenced)
- [x] Add cross-namespace collision detection (bind vs const/param/define)
- [x] Add unit tests (lexer, parser, resolve) and HIR/LIR snapshot tests

### Phase 2: Semantic Inference & Validation (Behavior Change) ✅

- [x] Implement bind direction inference (`->name` => out, `@name` only => in, otherwise E0311)
- [x] Implement bind contract inference (dtype/shape/rate from post-expansion graph via concrete actors)
- [x] Add diagnostics E0311 (bind unreferenced), E0312 (bind contract conflict: type/shape/rate mismatch)
- [x] Suppress E0023 for IN-bind buffers; pre-create BufferInfo with empty writer_task in resolve
- [x] Skip IN-bind buffers in LIR `build_inter_task_buffers()`; OUT-bind buffers retained for ring-buffer codegen
- [x] Thread `BindContract` into LIR (`LirBind.contract`) with Display showing direction/dtype/shape/rate
- [x] Add `resolve_port_shape()` to THIR (companion to `resolve_port_rate()`, returns individual dims)
- [x] Sorted iteration in all multi-reader inference loops for deterministic conflict diagnostics
- [x] Update spec diagnostic table (§10.6.4: E0300-E0312)
- [x] Unit tests (resolve, analyze: direction/dtype/shape/rate/unreferenced), snapshot tests (LIR bind-in/out)

### Phase 3: Stable ID & Interface Manifest ✅

- [x] Generate deterministic bind `stable_id` from graph-lineage CallIds (SHA-256 of direction + adjacent actor CallIds + transport, 16 hex chars)
- [x] Add `adjacent_actor_call_id()` graph helper (BFS from BufferWrite/BufferRead to adjacent Actor, handles modal tasks)
- [x] Thread `stable_id` into `BindContract` → `LirBind` → Display
- [x] Add `InterfaceManifest` struct with lossless ordered typed args (`InterfaceArg::Positional`/`Named`)
- [x] Add `LirProgram::generate_interface_manifest()` → JSON serialization
- [x] Add `--emit interface` (maps to `PassId::BuildLir`) and `--interface-out <path>` (orthogonal side-effect with terminal promotion)
- [x] Early-exit guard: `--interface-out` rejects `--emit manifest/build-info/ast`
- [x] Add determinism test (same source × 2 → identical stable_ids)
- [x] Add bind-reorder stability test (swapped bind declarations → same stable_ids)
- [x] Add topology-change detection test (different upstream actor → different stable_id)
- [x] Manifest snapshot test (`snapshot_lir_bind_interface_manifest`)

### Phase 4: Runtime Control Plane & Rebind ✅

- [x] Add `BindState`/`BindDesc` runtime structs with per-bind mutex for thread-safe endpoint access
- [x] Add runtime control-plane API (`list_bindings`, `rebind(stable_id, endpoint)`, `apply_pending_rebinds`)
- [x] Apply rebind atomically at iteration boundary via double-checked locking (per-iteration inside K-factor loop)
- [x] Add compiler `--bind name=endpoint` CLI flag with two-phase processing (parse before pipeline, validate after)
- [x] Add runtime `--bind name=endpoint` and `--list-bindings` introspection CLI flags
- [x] Codegen: emit BindState globals, BindDesc table, `_apply_pending_rebinds()`, ProgramDesc.binds wiring
- [x] Add `format_endpoint_spec()` and `endpoint_override` to interface manifest (with `skip_serializing_if`)
- [x] Precedence chain: DSL default < compiler `--bind` < runtime `--bind` < `rebind()`
- [x] Stage guard: reject `--bind` for non-effect stages (accept only `cpp|exe|interface` or with `--interface-out`)
- [x] Tests: 9 runtime bind tests (CLI, list, rebind, null guard), 2 LIR snapshot tests, codegen plumbing updates

### Phase 5: Codegen Lowering & Backward Compatibility ✅

- [x] Add `pipit_bind_io.h` runtime header: `BindIoAdapter` class with lazy-init PPKT send/recv, mutex protection, retry logic, reconnect
- [x] Add `extract_address()` free function for normalizing spec/raw endpoint strings at I/O time
- [x] Add diagnostic codes E0700-E0702 (transport/dtype/endpoint errors), W0700-W0701 (no-endpoint/unresolved-dtype warnings)
- [x] Codegen: resolve Ident endpoint args against `lir.consts`, transport+dtype compile-time guards
- [x] Codegen: emit `BindIoAdapter` instances, `send()` at `buf_write`, `recv()` at `buf_read`
- [x] Codegen: wire `reconnect()` in `_apply_pending_rebinds()` with correct lock ordering (`io_mtx_` → `state_->mtx`)
- [x] Keep `socket_write`/`socket_read` source compatibility for existing v0.3 programs
- [x] Add compatibility gate tests covering mixed mode (`bind` + legacy socket actors)

### Phase 6: Test & Acceptance Gate ✅

- [x] Add 11 codegen unit tests for bind adapter emission, send/recv wiring, reconnect, diagnostics
- [x] Add 4 integration compile tests (bind OUT/IN, mixed bind+socket, backward compat)
- [x] Add 11 runtime tests (`extract_address`, adapter no-op, loopback send/recv, zero-fill, reconnect, concurrent safety)
- [x] CI gate: `cargo fmt` + `cargo clippy -- -D warnings` + `cargo test` all green; `ctest` 12/12 pass

### Exit Criteria ✅

- [x] `bind`-only programs compile and run without explicit socket actor wiring
- [x] UI/client can enumerate bindings via `stable_id` and safely rebind at runtime
- [x] No regressions in existing v0.3 external I/O behavior and tests

---

## v0.4.3 - Shared-Memory Bind Transport (PSHM)

**Goal**: Implement `shm(...)` bind transport for multi-process local IPC, and align compiler/runtime behavior with optional interface-manifest output.

> **Priority rationale**: Highest performance impact — shared-memory IPC eliminates serialization and kernel-crossing overhead vs UDP. Critical for real-time multi-process pipelines. Depends on v0.4.2 Phases 1-3 (bind grammar, inference, stable ID) but Phase 2 (PSHM runtime core) can start in parallel.

### Phase 1: Compiler Surface Alignment

- [ ] Add `shm(name, slots, slot_bytes)` endpoint parsing/validation in bind endpoint grammar
- [ ] Add endpoint option range checks (`slots > 0`, `slot_bytes > 0`, bounded upper limits)
- [ ] Change interface manifest behavior from always-on to opt-in (`--emit interface`, `--interface-out <path>`)
- [ ] Keep `list_bindings` as mandatory runtime introspection path even when no manifest is emitted

### Phase 2: PSHM Runtime Core (Protocol v0.1.0) — parallelizable with v0.4.2

- [ ] Implement PSHM superblock + slot header binary layout exactly per spec (`magic/version/header_len/epoch/write_seq`)
- [ ] Implement single-writer publish path with release-store ordering guarantees
- [ ] Implement reader consume path with acquire-load ordering and overwrite detection
- [ ] Implement shared-memory object lifecycle (create/open/map/unmap/close) with safe defaults

### Phase 3: Contract Validation and Attach Semantics

- [ ] Validate attach-time contract (`dtype`, `shape`, `rate_hz`, `stable_id_hash`) against compiler-inferred bind contract
- [ ] Reject mismatched endpoints at startup with clear diagnostics (startup error path)
- [ ] Define and implement endpoint precedence (`CLI --bind` override vs DSL bind default)

### Phase 4: Rebind Epoch Semantics

- [ ] Implement iteration-boundary rebind apply point for PSHM endpoints
- [ ] Emit/consume epoch fence markers during endpoint generation switch
- [ ] Ensure reader resynchronization drops incomplete frame state across epoch transition

### Phase 5: Codegen and Runtime Wiring

- [ ] Lower `bind ... = shm(...)` to runtime PSHM adapter wiring in generated C++
- [ ] Keep existing UDP/Unix datagram bind paths behaviorally unchanged
- [ ] Preserve `socket_write`/`socket_read` backward compatibility while bind transport backends coexist

### Phase 6: Verification, Tests, and Performance

- [ ] Add struct layout tests (Superblock=128B, SlotHeader=64B, field offsets)
- [ ] Add protocol tests for sequence monotonicity, lag overwrite handling, and non-blocking empty read
- [ ] Add integration tests for tx.pdl/rx.pdl cross-process shared-memory loopback
- [ ] Add rebind tests validating epoch fence and atomic boundary switch behavior
- [ ] Add determinism tests for stable_id and optional interface manifest reproducibility
- [ ] Measure throughput/latency vs UDP bind baseline and record report under `doc/performance/`

### Exit Criteria

- [ ] Two independent PDL executables can exchange data via `bind ... = shm(...)` on one host
- [ ] Rebind over PSHM is atomic at iteration boundary with no mixed-epoch frame visibility
- [ ] Interface manifest is optional; runtime `list_bindings` provides equivalent bind metadata
- [ ] No regressions in existing UDP/Unix bind behavior or legacy socket actor behavior

---

## v0.4.4 - Compiler Latency Profiling, Deterministic Caching & Recovery

**Goal**: Measure and recover compile-latency regression; add deterministic artifact caching without changing compiler semantics.

> **Priority rationale**: Medium criticality — compiler latency is a developer-experience concern, not a runtime performance issue. Regression was observed post-v0.4.0 architecture rebuild; recovery should be measurement-driven. Deterministic keys (formerly in memory plan) belong here as they enable artifact caching.

### Deterministic Artifact Keys

- [ ] Implement deterministic `invalidation_key` hashing for pass artifacts
- [ ] Integrate manifest/header provenance into cache keys and diagnostics
- [ ] Add artifact hashing and reusable cache for heavy phases

### Profiling & Regression Recovery

- [ ] Run formal KPI A/B benchmark against v0.3.4 baseline (`compiler_bench_stable.sh --baseline-ref v0.3.4`)
- [ ] Record release disposition for compile-latency regression after benchmark review
- [ ] Profile per-phase time (`build_hir`, `build_thir`, `build_lir`, `codegen`) and rank dominant costs
- [ ] Reduce allocation/clone overhead in `build_lir`
- [ ] Evaluate lazy/on-demand LIR field materialization for codegen-only paths
- [ ] Audit `precompute_metadata()` duplication against analysis-owned data
- [ ] Re-measure after each optimization; target per-scenario latency within 10% of `7248b44`

---

## v0.4.5 - Diagnostics Completion

**Goal**: Complete diagnostics provenance and ambiguity guidance to improve debuggability and remediation clarity.

> **Priority rationale**: Lowest urgency in v0.4.x — no performance impact, no blocking dependency. Quality-of-life improvement for users debugging compilation failures.

- [ ] Add full provenance tracing through the constraint solver
- [ ] Improve ambiguity/mismatch diagnostics with candidate and remediation suggestions

---

## v0.5.x - Ecosystem & Quality of Life

**Goal**: Make Pipit easier to use and deploy in real projects.

> **Status**: Deferred. All unchecked (`- [ ]`) items in this `v0.5.x` section are deferred.

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
- **v0.4.0 summary**: architecture rebuild completed across contract freeze, IR unification, pass-manager orchestration, verification/diagnostics upgrade, runtime-shell extraction, registry determinism, and migration hardening.
- **v0.4.0 delivered artifacts**: HIR/THIR/LIR production pipeline, `codegen_from_lir` path, unified diagnostics with stable codes in `pcc-spec-v0.4.0.md` §10.4-§10.6, `--emit manifest` / `--emit build-info`, and manifest-first CMake integration.
- **v0.4.x deferred work placement**: follow-up items from v0.4.0 are grouped into `v0.4.1` through `v0.4.5` by priority (criticality → performance impact → complexity). Deterministic artifact keys moved from memory plan to v0.4.4 (compiler caching).
- **v0.4.1 summary**: MemoryKind classification (ADR-028), SPSC ring buffer (ADR-029), param sync simplification (ADR-030), `alignas(64)` edge buffers, `--experimental` flag. Audited for over-engineering; scalarization/assume_aligned/locality-scoring deferred.
- **v0.4.1 ADRs**: ADR-028 (edge memory classification), ADR-029 (SPSC ring buffer specialization), ADR-030 (param sync simplification)
- **v0.4.2 Phase 4 summary**: Runtime control plane with `BindState`/`BindDesc` structs, double-checked locking for `rebind()`/`apply_pending_rebinds()`, compiler `--bind` flag (reject-based stage guard, two-phase processing), runtime `--bind`/`--list-bindings` CLI, per-iteration rebind apply in K-factor loop, interface manifest `endpoint_override` field. 9 runtime + 2 LIR snapshot tests added.
- **v0.4.2 Phase 5 summary**: Codegen lowering via `BindIoAdapter` runtime class (`pipit_bind_io.h`): lazy-init PPKT send/recv, mutex-protected I/O, retry logic (3 attempts), reconnect with correct lock ordering. Codegen emits adapter instances, `send()`/`recv()` at buffer write/read points, `reconnect()` in `_apply_pending_rebinds()`. Compile-time transport guard (udp/unix_dgram only, E0700), dtype guard (PPKT mapping, E0701), Ident endpoint resolution (E0702). `extract_address()` normalizes spec/raw endpoints at I/O time. 26 new tests (11 codegen unit + 4 integration compile + 11 runtime). Full backward compatibility with socket actors verified.
- **v0.5.x** open items are currently deferred
- Performance characterization should inform optimization priorities (measure before optimizing)
- Spec files renamed to versioned names (`pipit-lang-spec-v0.3.0.md`, `pcc-spec-v0.3.0.md`); `v0.2.0` specs are frozen from tag `v0.2.2`
