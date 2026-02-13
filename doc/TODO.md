# Pipit Development TODO

Based on [pipit-lang-spec-v0.1.0](spec/pipit-lang-spec-v0.1.0.md).

---

## Project Scaffold
- [x] Decide implementation language for `pcc` compiler → Rust ([ADR-001](adr/001-rust-for-pcc.md))
- [x] Set up build system and project structure → `compiler/` (Rust) + `runtime/` (C++)
- [ ] CI pipeline: format → lint → typecheck → test
- [x] Create `doc/adr/` for architecture decisions

## Lexer (§2)
- [ ] UTF-8 source reading
- [ ] Tokenize keywords: `set const param define clock mode control switch default delay`
- [ ] Tokenize special symbols: `| -> @ : ? $ #`
- [ ] Numeric literals (int, float, negative, exponent)
- [ ] Unit literals: frequency (`Hz kHz MHz GHz`), size (`KB MB GB`)
- [ ] String literals with `\"` `\\` escapes
- [ ] Array literals `[...]`
- [ ] Identifiers `[a-zA-Z_][a-zA-Z0-9_]*`
- [ ] Line comments `#`
- [ ] Error reporting with source location

## Parser & AST (§10 BNF)
- [ ] Define AST node types for all grammar productions
- [ ] `set_stmt`, `const_stmt`, `param_stmt`
- [ ] `define_stmt` (sub-pipeline)
- [ ] `task_stmt` (`clock <freq> <name> { ... }`)
- [ ] `pipeline_body`: pipe expressions with source / elem / sink
- [ ] `actor_call` with args (scalar, `$param`, const ref)
- [ ] Tap `:name`, probe `?name`, buffer read `@name`, buffer write `-> name`
- [ ] `modal_body`: `control`, `mode`, `switch` with `default` clause
- [ ] Syntax error recovery and diagnostics

## Actor Registry Interface (§4)
- [ ] Define actor metadata schema (name, in/out type, token counts, params)
- [ ] Parse `ACTOR` macro `constexpr` registration info from C++ headers
- [ ] Support `PARAM` and `RUNTIME_PARAM` metadata extraction
- [ ] Actor lookup by name

## Name Resolution (§8 step 3)
- [ ] Resolve actor names against registry
- [ ] Resolve `const` and `param` references
- [ ] Resolve shared buffer names (`->` define, `@` reference)
- [ ] Resolve tap names (`:name` declare vs consume, task scope)
- [ ] Resolve `$name` runtime parameter references
- [ ] Name collision detection within same namespace
- [ ] Diagnostics: unknown name, unused tap, duplicate definition

## SDF Graph Construction (§8 step 4)
- [ ] Build directed graph from pipeline expressions
- [ ] Expand taps to fork nodes (`IN(T,N) → OUT(T,N) × M`)
- [ ] Inline-expand `define` sub-pipelines
- [ ] Convert shared buffers to inter-task edges
- [ ] Feedback loop detection

## Static Analysis (§8 step 5)
- [ ] Type checking: verify pipe endpoint type compatibility (§3)
- [ ] SDF balance equation solving → repetition vector (§5.5)
- [ ] Feedback loop `delay` verification (§5.10)
- [ ] Cross-clock rate matching: `Pw × fw = Cr × fr` (§5.7)
- [ ] Single-writer constraint on shared buffers (§5.7)
- [ ] Tap consumption check: declared taps must be consumed (§5.6)
- [ ] Buffer size computation (safe upper bound) (§5.7)
- [ ] Memory pool check vs `set mem` limit
- [ ] `param` type vs `RUNTIME_PARAM` type match

## CSDF Mode Analysis (§6)
- [ ] Build control subgraph as independent SDF graph
- [ ] Build each `mode` block as independent SDF graph
- [ ] Validate `switch` ctrl supplier exists (control block or param)
- [ ] Validate ctrl type is `int32`
- [ ] Validate mode index coverage (0 .. N-1)
- [ ] Per-mode balance equation solving and buffer sizing

## Schedule Generation (§8 step 6)
- [ ] Per-task topological order (PASS construction)
- [ ] Determine K (iterations per tick) from target rate
- [ ] Batching optimization for high target rates

## C++ Code Generation (§8 step 7)
- [ ] Ring buffer static allocation code
- [ ] Per-task schedule loop (actor firing sequence)
- [ ] Runtime parameter double-buffering mechanism
- [ ] Overrun detection and policy (`drop`, `slip`, `backlog`)
- [ ] Probe instrumentation (stripped in `--release`)
- [ ] CSDF mode transition logic (ctrl evaluation, mode swap at iteration boundary)
- [ ] `main()` with CLI argument parser
- [ ] Statistics collection code (`--stats`)

## Runtime Library — `libpipit`
- [ ] Ring buffer (shared memory, lock-free SPSC)
- [ ] Scheduler: `static` and `round_robin` strategies
- [ ] Timer / tick generator (OS timer abstraction)
- [ ] Overrun policies: drop, slip, backlog
- [ ] Double-buffering for runtime parameters (atomic swap at iteration boundary)
- [ ] Thread management (task → thread mapping)
- [ ] Statistics collection and reporting
- [ ] Signal handling (SIGINT → graceful shutdown)

## CLI & Integration (§9)
- [ ] CLI flags: `--duration`, `--threads`, `--param`, `--probe`, `--probe-output`, `--stats`
- [ ] Exit codes: 0 (normal), 1 (runtime error), 2 (startup error)
- [ ] End-to-end test: spec §11.2 `example.pdl` compiles and runs
- [ ] End-to-end test: spec §11.3 `receiver.pdl` compiles and runs
- [ ] Error message quality review (match spec §7 examples)

## Polish & Release Prep
- [ ] Compiler error messages match spec format (§7.1)
- [ ] Runtime error propagation matches spec (§7.2)
- [ ] `--release` build strips probes to zero cost
- [ ] Documentation: `pcc` usage guide
- [ ] Performance: compile-time benchmarks on non-trivial graphs
