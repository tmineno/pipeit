# Pipit Development TODO

Based on [pipit-lang-spec-v0.1.0](spec/pipit-lang-spec-v0.1.0.md).

---

## Project Scaffold

- [x] Decide implementation language for `pcc` compiler → Rust ([ADR-001](adr/001-rust-for-pcc.md))
- [x] Set up build system and project structure → `compiler/` (Rust) + `runtime/` (C++)
- [x] CI pipeline: format → lint → typecheck → test → `.github/workflows/ci.yml`
- [x] Create `doc/adr/` for architecture decisions
- [x] Document shared buffer multi-reader semantics ([ADR-006](adr/006-shared-buffer-multi-reader-ring.md))

## Lexer (§2)

- [x] UTF-8 source reading
- [x] Tokenize keywords: `set const param define clock mode control switch default delay`
- [x] Tokenize special symbols: `| -> @ : ? $ ( ) { } [ ] , =`
- [x] Numeric literals (int, float, negative, exponent)
- [x] Unit literals: frequency (`Hz kHz MHz GHz`), size (`KB MB GB`)
- [x] String literals with `\"` `\\` escapes
- [x] Array literals `[...]` — brackets tokenized; array parsing deferred to parser
- [x] Identifiers `[a-zA-Z_][a-zA-Z0-9_]*`
- [x] Line comments `#`
- [x] Error reporting with source location

## Parser & AST (§10 BNF)

- [x] Define AST node types for all grammar productions
- [x] `set_stmt`, `const_stmt`, `param_stmt`
- [x] `define_stmt` (sub-pipeline)
- [x] `task_stmt` (`clock <freq> <name> { ... }`)
- [x] `pipeline_body`: pipe expressions with source / elem / sink
- [x] `actor_call` with args (scalar, `$param`, const ref)
- [x] Tap `:name`, probe `?name`, buffer read `@name`, buffer write `-> name`
- [x] `modal_body`: `control`, `mode`, `switch` with `default` clause
- [x] Syntax error recovery and diagnostics

## Actor Registry Interface (§4)

- [x] Define actor metadata schema (name, in/out type, token counts, params)
- [x] Parse `ACTOR` macro `constexpr` registration info from C++ headers
- [x] Support `PARAM` and `RUNTIME_PARAM` metadata extraction
- [x] Actor lookup by name

## Name Resolution (§8 step 3)

- [x] Resolve actor names against registry
- [x] Resolve `const` and `param` references
- [x] Resolve shared buffer names (`->` define, `@` reference)
- [x] Resolve tap names (`:name` declare vs consume, task scope)
- [x] Resolve `$name` runtime parameter references
- [x] Name collision detection within same namespace
- [x] Diagnostics: unknown name, unused tap, duplicate definition

## SDF Graph Construction (§8 step 4)

- [x] Build directed graph from pipeline expressions
- [x] Expand taps to fork nodes (`IN(T,N) → OUT(T,N) × M`)
- [x] Inline-expand `define` sub-pipelines
- [x] Convert shared buffers to inter-task edges
- [x] Feedback loop detection

## Static Analysis (§8 step 5)

- [x] Type checking: verify pipe endpoint type compatibility (§3)
- [x] SDF balance equation solving → repetition vector (§5.5)
- [x] Feedback loop `delay` verification (§5.10)
- [x] Cross-clock rate matching: `Pw × fw = Cr × fr` (§5.7)
- [x] Single-writer constraint on shared buffers (§5.7)
- [x] Tap consumption check: declared taps must be consumed (§5.6)
- [x] Buffer size computation (safe upper bound) (§5.7)
- [x] Memory pool check vs `set mem` limit
- [x] `param` type vs `RUNTIME_PARAM` type match

## CSDF Mode Analysis (§6)

- [x] Build control subgraph as independent SDF graph
- [x] Build each `mode` block as independent SDF graph
- [x] Validate `switch` ctrl supplier exists (control block or param)
- [x] Validate ctrl type is `int32`
- [x] Validate mode index coverage (0 .. N-1)
- [x] Per-mode balance equation solving and buffer sizing

## Schedule Generation (§8 step 6)

- [x] Per-task topological order (PASS construction via Kahn's algorithm)
- [x] Determine K (iterations per tick) from target rate
- [x] Batching optimization for high target rates (K = ceil(freq / 1MHz))
- [x] Intra-task buffer sizing per edge
- [x] Feedback cycle back-edge identification (delay actor breaks cycle)
- [x] `--emit schedule` output for debugging

## C++ Code Generation (§8 step 7)

- [x] Ring buffer static allocation code
- [x] Per-task schedule loop (actor firing sequence)
- [x] Runtime parameter double-buffering mechanism
- [x] Overrun detection and policy (`drop`, `slip`, `backlog`)
- [x] Probe instrumentation (stripped in `--release`)
- [x] CSDF mode transition logic (ctrl evaluation, mode swap at iteration boundary)
- [x] `main()` with CLI argument parser
- [x] Statistics collection code (`--stats`)
- [x] Actor error propagation (check `operator()` return, exit code 1)
- [x] Functional probe output (`fprintf` when `--probe` enabled, `#ifndef NDEBUG`)

## Runtime Library — `libpipit`

- [x] Ring buffer (shared memory, lock-free SPSC with multi-reader support)
  - [x] Multi-reader FIFO with independent read cursors ([ADR-006](adr/006-shared-buffer-multi-reader-ring.md))
  - [x] Per-reader tail tracking for capacity calculation
  - [x] Status-checking read/write API for fail-fast semantics
- [x] Scheduler: `static` strategy (one thread per task, current implementation)
  - [ ] Future: `round_robin` strategy with thread pool (deferred to v0.2)
- [x] Timer / tick generator (OS timer abstraction)
- [x] Overrun policies: drop, slip, backlog (Timer: `last_latency`, `missed_count`, `reset_phase`)
- [x] Double-buffering for runtime parameters (atomic swap at iteration boundary)
- [x] Thread management (task → thread mapping)
- [x] Statistics collection and reporting (`pipit::TaskStats`)
- [x] Signal handling (SIGINT → graceful shutdown)

## CLI & Integration (§9)

- [x] Compiler CLI:
  - [x] `-o/--output` for explicit output file specification
  - [x] `--actor-path` for automatic actor header discovery
  - [x] `--verbose` for phase timing and diagnostics
  - [x] `--cc` and `--cflags` for C++ compiler customization
  - [x] Exit codes: 0 (success), 1 (compile error), 2 (usage error), 3 (system error)
- [x] Generated binary CLI:
  - [x] Runtime flags: `--duration`, `--threads`, `--param`, `--probe`, `--probe-output`, `--stats`
  - [x] Exit codes: 0 (normal), 1 (runtime error), 2 (startup error)
  - [x] Improved error messages with "startup error:" prefix and context
  - [x] Robust duration parsing with time suffixes (`10s`, `1m`, `inf`)
  - [x] Input validation for all flags (missing args, invalid values)
- [x] End-to-end tests:
  - [x] Spec §11.2 `example.pdl` compiles and runs
  - [x] Spec §11.3 `receiver.pdl` compiles and runs
  - [x] Overrun policy tests (drop/slip/backlog)
  - [x] CLI flag tests (exit codes, stats output, duration suffixes)
- [x] Error message quality review (match spec §7 examples, hints in Diagnostic)

## Visualization

- [x] SDF graph visualization output (`--emit graph-dot` produces Graphviz DOT format)
  - [x] Task clusters with nested modal control/mode subgraphs
  - [x] Node shapes per kind (box=actor, diamond=fork, circle=probe, cylinder=buffer)
  - [x] Probe rendered as side-branch off main dataflow
  - [x] Inter-task buffer edges (dashed red)
  - [x] Feedback cycle edges (bold blue)
  - [x] Deterministic output (sorted tasks, namespaced node IDs)
- [x] Timing diagram visualization (`--emit timing-chart` produces Mermaid Gantt chart)
  - [x] ASAP parallel scheduling (independent branches run concurrently)
  - [x] Mermaid-safe labels (fork, probe, buffer nodes avoid `:` separator conflicts)
  - [x] Zero-duration probes omitted from output
  - [x] Numeric cycle axis (`dateFormat x` + `axisFormat %Q`)
  - [x] Feedback back-edges excluded from timing dependencies
  - [x] Modal task sections (control + per-mode, modes start at offset 0)
  - [x] Deterministic output (sorted tasks, unique task IDs)

## Polish & Release Prep

- [x] Compiler error messages match spec format (§7.1, hints in Diagnostic)
- [x] Runtime error propagation matches spec (§7.2, actor ACTOR_ERROR → exit code 1)
- [x] `--release` build strips probes to zero cost (`#ifndef NDEBUG`)
- [x] Documentation: `pcc` usage guide (`doc/pcc-usage-guide.md`)
- [x] Performance: compile-time benchmarks on non-trivial graphs
  - [x] Compiler benchmarks (parse, full pipeline, codegen) - `compiler/benches/compiler_bench.rs`
  - [x] Runtime benchmarks (RingBuffer, Timer, TaskStats) - `benches/runtime_bench.cpp`
  - [x] Baseline metrics: simple (3.5µs), medium (6.6µs), complex (11µs) parse times
