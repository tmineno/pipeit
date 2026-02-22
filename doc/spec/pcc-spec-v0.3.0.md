# Feature: pcc — Pipit Compiler Collection

Version: 0.3.0

## 1. Goal

`pcc` is the compiler for Pipit Definition Language (`.pdl`) files. It reads a pipeline description and actor metadata inputs (`actors.meta.json` or scanned actor headers), performs static analysis, and produces either a standalone executable or generated C++ source code.

Refer to [pipit-lang-spec](pipit-lang-spec-v0.3.0.md) for the full language semantics. This spec defines `pcc`'s behavior as a tool.

## 2. Non-goals

- `pcc` does not parse C++ actor bodies. Actor metadata is loaded from `actors.meta.json` (`--actor-meta`) or from direct header scanning (`-I` / `--include` / `--actor-path`).
- `pcc` does not provide an incremental/watch mode (full recompilation only)
- `pcc` does not manage external C++ dependencies beyond `libpipit`
- IDE integration (LSP) is out of scope for v0.3.x

---

## 3. Implementation

`pcc` is implemented in Rust (see [ADR-001](../adr/001-rust-for-pcc.md)). The generated C++ code links against the `libpipit` runtime library (C++17). There is no Rust–C++ FFI; `pcc` reads actor metadata from manifest files and invokes the system C++ compiler as a subprocess.

---

## 4. Software Architecture

```
                         ┌─────────────────────────────────────────────┐
                         │              pcc (Rust binary)              │
                         │                                             │
  ┌──────────┐           │  ┌─────────┐   ┌───────────────────────┐   │
  │ .pdl     │──────────▶│  │  Lexer  │──▶│  Parser               │   │
  │ source   │           │  └─────────┘   │  (AST)     [emit ast] │   │
  └──────────┘           │                └───────────┬───────────┘   │
                         │                            │               │
  ┌──────────┐           │  ┌─────────────────────┐   │               │
  │ actors.h │──────────▶│  │  Actor Registry     │   │               │
  │ (-I)     │           │  │  (metadata reader)  │───┤               │
  └──────────┘           │  └─────────────────────┘   │               │
                         │                            ▼               │
                         │                ┌───────────────────────┐   │
                         │                │  Name Resolution      │   │
                         │                └───────────┬───────────┘   │
                         │                            ▼               │
                         │                ┌───────────────────────┐   │
                         │                │  SDF Graph Builder    │   │
                         │                │           [emit graph]│   │
                         │                └───────────┬───────────┘   │
                         │                            ▼               │
                         │                ┌───────────────────────┐   │
                         │                │  Static Analysis      │   │
                         │                │  (types, rates,       │   │
                         │                │   balance, buffers)   │   │
                         │                └───────────┬───────────┘   │
                         │                            ▼               │
                         │                ┌───────────────────────┐   │
                         │                │  Schedule Generator   │   │
                         │                └───────────┬───────────┘   │
                         │                            ▼               │
                         │                ┌───────────────────────┐   │   ┌──────────────┐
                         │                │  C++ Code Generator   │───┼──▶│ *_gen.cpp    │
                         │                │           [emit cpp]  │   │   │ [emit cpp]   │
                         │                └───────────────────────┘   │   └──────────────┘
                         └────────────────────────┬───────────────────┘
                                                  │ [emit exe]
                                                  │ invokes subprocess
                                                  ▼
                         ┌─────────────────────────────────────────────┐
                         │          System C++ Compiler (--cc)         │
                         │          g++ / clang++ -std=c++17 -lpthread │
                         └────────────────────────┬────────────────────┘
                                                  │
                              ┌────────────────┐  │  ┌────────────────┐
                              │ libpipit       │──┘  │                │
                              │ (C++ runtime)  │────▶│  executable    │
                              └────────────────┘     └────────────────┘
```

### 4.1 Module mapping (Rust crate: `pcc`)

| Module | Responsibility |
|--------|---------------|
| `lexer` | Tokenize `.pdl` source (§2) |
| `parser` | Build AST from token stream (§10 BNF) |
| `registry` | Load actor metadata from manifest (`--actor-meta`) or direct header scanning (`-I` / `--include` / `--actor-path`) and build ActorRegistry (§8) |
| `resolve` | Name resolution — actors, consts, params, buffers, taps |
| `type inference / monomorphization` | Solve type constraints, apply safe widening, instantiate polymorphic actors |
| `ir` | Build typed/lowered IR consumed by all downstream phases |
| `lowering verifier` | Check proof obligations (type/rate/shape preservation) for lowering |
| `graph` | SDF graph construction — tap expansion, define inlining, inter-task edges |
| `analyze` | Static analysis — type check, balance equations, buffer sizing, CSDF |
| `schedule` | Schedule generation — topological order, K determination, batching, rate-domain fusion planning |
| `codegen` | C++ code emission from typed scheduled IR (no type re-inference) |
| `timing` | Timing chart emission (`--emit timing-chart`) |

### 4.2 Data flow between modules

```
.pdl source ─▶ Lexing/Parsing ─▶ AST
                                 │
actors.meta.json or actor headers ─▶ Actor Registry
                                 │
                                 ▼
                          Name Resolution
                                 │
                                 ▼
                  Type Inference & Monomorphization
                                 │
                                 ▼
                   Typed Lowering + Verification
                                 │
                                 ▼
                      SDF Graph Construction
                                 │
                                 ▼
                          Static Analysis
                                 │
                                 ▼
                        Schedule Generation
                                 │
                                 ▼
                          C++ Code Generation
                                 │
                                 ▼
                              C++ source
```

Note: This diagram is conceptual phase flow, not a direct module API/signature specification. Actor headers (`-I` / `--include`, `--actor-path`) are fallback registry inputs when `--actor-meta` is omitted, and are also used as C++ declaration inputs for generated code.

---

## 5. Inputs

### 5.1 Source file (required)

A single `.pdl` file containing the pipeline description.

```
pcc example.pdl [OPTIONS]
```

### 5.2 Actor metadata manifest (`--actor-meta`)

A JSON manifest containing serialized actor metadata (`schema = 1`, `actors[]`), consumed by `registry::Manifest`.

```
pcc example.pdl --actor-meta ./build/actors.meta.json
```

If omitted, `pcc` scans actor headers from `-I` / `--include` / `--actor-path` directly to build the registry.

`--actor-meta` provides analysis metadata only. For `--emit exe`, actor declarations are still required through headers (`-I` / `--include` or `--actor-path`) so generated C++ can compile.

### 5.3 Actor headers (`-I`, `--include`) — registry fallback and C++ declaration input

One or more paths (header files or directories) containing `ACTOR` macro definitions.
When no `--actor-meta` is provided, these inputs are scanned directly for actor metadata.

```
pcc example.pdl -I ./actors.h -I ./extra_actors.h
```

### 5.4 Actor search path (`--actor-path`)

Directories recursively searched for actor headers.

```
pcc example.pdl --actor-path ./actors/ --actor-path /usr/include/pipit/
```

`-I` takes precedence over `--actor-path` when the same actor name is found in both.

---

## 6. Outputs

### 6.1 Default: Executable

By default, `pcc` generates C++ source code, invokes the system C++ compiler, and links against `libpipit` to produce an executable.

```
pcc example.pdl --actor-meta build/actors.meta.json -I actors.h -o receiver
# produces: ./receiver
```

### 6.2 `--emit cpp`: Generated C++ source only

Stop after code generation without invoking the C++ compiler. Useful for inspection and custom build integration.

```
pcc example.pdl -I actors.h --emit cpp -o receiver_gen.cpp
# produces: receiver_gen.cpp
```

### 6.3 `--emit ast`: AST dump

Dump the parsed AST in a human-readable format. For debugging the compiler frontend.

```
pcc example.pdl --emit ast
```

### 6.4 `--emit graph`: SDF graph dump

Dump the constructed SDF graph with repetition vectors and buffer sizes. For debugging the analysis phase.

```
pcc example.pdl -I actors.h --emit graph
```

### 6.5 `--emit graph-dot`: Graphviz DOT dump

Emit Graphviz DOT format for visualization tooling.

```
pcc example.pdl -I actors.h --emit graph-dot
```

### 6.6 `--emit schedule`: PASS schedule dump

Emit firing order and repetition counts after scheduling.

```
pcc example.pdl -I actors.h --emit schedule
```

### 6.7 `--emit timing-chart`: Mermaid Gantt timing chart

Emit a Mermaid Gantt chart derived from schedule + graph.

```
pcc example.pdl -I actors.h --emit timing-chart
```

---

## 7. CLI Interface

```
pcc <source.pdl> [OPTIONS]
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-o <path>` | PATH | `a.out` | Output file path |
| `--actor-meta <file>` | PATH | — | Actor metadata manifest (`actors.meta.json`) |
| `-I, --include <path>` | PATH (repeatable) | — | Actor header file or search directory |
| `--actor-path <dir>` | PATH (repeatable) | — | Actor search directory |
| `--emit <stage>` | `exe` \| `cpp` \| `ast` \| `graph` \| `graph-dot` \| `schedule` \| `timing-chart` | `exe` | Output stage |
| `--release` | flag | off | Release build: strip probes, enable optimizations |
| `--cc <compiler>` | STRING | `c++` | C++ compiler command |
| `--cflags <flags>` | STRING | mode-dependent (`-O0 -g` debug, `-O2` release) | Additional C++ compiler flags |
| `--verbose` | flag | off | Print compiler phases and timing |
| `--version` | flag | — | Print version and exit |
| `--help` | flag | — | Print usage and exit |

### 7.1 Release vs Debug build

| Behavior | Debug (default) | Release (`--release`) |
|----------|-----------------|----------------------|
| Probe instrumentation | Included | Stripped (zero cost) |
| C++ optimization | `-O0 -g` | `-O2` |
| C++ standard | `-std=c++17` | `-std=c++17` |
| Runtime assertions | Enabled | Disabled |

When `--cflags` is explicitly provided, it overrides the default optimization flags for both modes.

---

## 8. Actor Metadata Loading

`pcc` loads actor metadata from either a manifest file or scanned headers. Resolution order:

1. If `--actor-meta` is provided, load it directly.
2. Otherwise, scan headers from `-I` / `--include` and `--actor-path` directly.
3. Validate manifest schema version and required fields.
4. Build an actor registry keyed by name (polymorphic actors keyed by base name).

Manifest schema (minimum):

```json
{
  "schema": 1,
  "actors": [
    {
      "name": "fir",
      "type_params": ["T"],
      "in_type": { "TypeParam": "T" },
      "in_count": { "Symbolic": "N" },
      "in_shape": { "dims": [ { "Symbolic": "N" } ] },
      "out_type": { "TypeParam": "T" },
      "out_count": { "Literal": 1 },
      "out_shape": { "dims": [ { "Literal": 1 } ] },
      "params": [
        { "kind": "Param", "param_type": { "SpanTypeParam": "T" }, "name": "coeff" },
        { "kind": "Param", "param_type": "Int", "name": "N" }
      ]
    }
  ]
}
```

Duplicate actor names in the loaded manifest are a compile error:

```
error: duplicate actor 'fft': first defined in ./actors.meta.json, redefined in ./actors.meta.json
```

Invalid manifest diagnostics:

```text
error: invalid actor metadata schema (expected: 1, found: 0)
error: invalid manifest JSON: missing field `in_type` at line ...
```

---

## 9. Compilation Phases

`pcc` executes the following phases sequentially. A failure in any phase halts compilation.

```
source.pdl + actor metadata inputs
  │
  ├─ 1. Lex & Parse                          [--emit ast]
  │     └─ Tokenize .pdl source, build AST
  │
  ├─ 2. Actor Loading
  │     └─ Load actor metadata and build registry
  │        (name, in/out type, token count/shape, params, type params)
  │
  ├─ 3. Name Resolution
  │     ├─ Actor names: lookup in registry (parenthesized only)
  │     ├─ Shared buffer names: -> defines, @ references
  │     ├─ Tap names: : defines/references (task scope)
  │     ├─ const / param names: global scope
  │     └─ Collision detection (no duplicates in same namespace)
  │
  ├─ 4. Type Inference & Monomorphization
  │     ├─ Solve actor/pipeline type constraints
  │     ├─ Apply safe numeric widening (int8->...->int32->float->double, cfloat->cdouble)
  │     ├─ Resolve polymorphic actor calls
  │     └─ Materialize concrete actor instances
  │
  ├─ 5. Typed Lowering + Verification
  │     ├─ Rewrite to explicit lowered IR (insert widening nodes)
  │     ├─ Emit lowering certificate (Cert)
  │     └─ Verify proof obligations before proceeding
  │
  ├─ 6. SDF Graph Construction               [--emit graph, --emit graph-dot]
  │     ├─ Expand taps to fork nodes
  │     ├─ Inline-expand define sub-pipelines
  │     ├─ Convert shared buffers to inter-task edges
  │     ├─ Build control subgraph as independent sub-graph
  │     └─ Build each mode block as independent sub-graph
  │
  ├─ 7. Static Analysis
  │     ├─ Type compatibility at pipe endpoints
  │     ├─ SDF balance equation solving → repetition vector
  │     ├─ Feedback loop delay verification
  │     ├─ Cross-clock rate matching (Pw×fw = Cr×fr)
  │     ├─ CSDF per-mode analysis
  │     ├─ ctrl supplier verification
  │     └─ Buffer size computation (safe upper bound)
  │
  ├─ 8. Schedule Generation                  [--emit schedule, --emit timing-chart]
  │     ├─ Per-task topological order (PASS construction)
  │     ├─ K (iterations/tick) determination
  │     ├─ Rate-domain (fusion-domain) detection
  │     └─ Batching optimization for high target rates
  │
  ├─ 9. C++ Code Generation                  [--emit cpp]
  │     ├─ Ring buffer static allocation
  │     ├─ Per-task schedule loop (from TypedScheduledIR)
  │     ├─ Runtime parameter double-buffering
  │     ├─ Overrun detection and statistics
  │     ├─ No type fallback / no type re-inference
  │     ├─ Probe instrumentation (stripped in --release)
  │     └─ main() with CLI argument parser
  │
  └─ 10. C++ Compilation                     [--emit exe]
        └─ Invoke cc -std=c++17 (-O0/-O2) -lpthread → executable
```

| Phase | Description | Can emit here |
|-------|-------------|---------------|
| 1. Lex & Parse | Tokenize `.pdl`, build AST | `--emit ast` |
| 2. Actor Loading | Load actor metadata and build registry | |
| 3. Name Resolution | Resolve actors, consts, params, buffers, taps | |
| 4. Type Inference & Monomorphization | Resolve polymorphic calls and safe widening | |
| 5. Typed Lowering + Verification | Lower polymorphism/implicit widening to explicit IR and verify obligations | |
| 6. SDF Graph Construction | Expand taps, inline defines, build inter-task edges | `--emit graph`, `--emit graph-dot` |
| 7. Static Analysis | Type check, balance equations, buffer sizing | |
| 8. Schedule Generation | Topological order, K determination, rate-domain detection, batching | `--emit schedule`, `--emit timing-chart` |
| 9. C++ Code Generation | Emit C++ source from TypedScheduledIR (no re-inference) | `--emit cpp` |
| 10. C++ Compilation | Invoke system compiler, link `libpipit` | `--emit exe` |

### 9.1 Rate-domain (fusion-domain) optimization

To reduce loop overhead while preserving SDF semantics, `pcc` MAY group adjacent firings into a `rate domain` and emit a fused inner loop.

`rate domain` (aka `fusion domain`) is an implementation-level schedule unit and is not a DSL construct.

#### 9.1.1 Eligibility (all required)

- Same task and same subgraph (`pipeline`, `control`, or one mode subgraph)
- Adjacent in topological firing order
- Equal repetition count
- Single-rate compatibility along the grouped path (per-firing token transfer matches on each grouped edge)
- No feedback back-edge crossing (`delay` cycle cut edges are barriers)
- No barrier nodes that require standalone placement (e.g., shared-buffer read/write boundaries)

#### 9.1.2 Correctness constraints

When fusion is applied, generated code MUST preserve:

- Iteration semantics from the language spec (one iteration still fires each actor exactly `rep(node)` times)
- Per-edge FIFO token order
- Observable side-effect order (`stdout`, file/network sinks, probes, shared-buffer write/read order)
- Existing error behavior (`ACTOR_ERROR` short-circuits task execution exactly as before)

#### 9.1.3 Non-requirement

Fusion is optional. If eligibility is not met, `pcc` may emit the unfused per-node loop form.

---

### 9.2 Typed IR and Verified Lowering (Normative)

To prevent semantic drift introduced by polymorphism and implicit widening,
`pcc` MUST perform lowering with explicit proof obligations.

#### 9.2.1 Lowering contract

Lowering is defined as:

```
Lower(G_typed) -> (G_lowered, Cert)
```

- `G_typed`: graph after type inference / monomorphization
- `G_lowered`: graph where all implicit widening is explicit and all actors are concrete
- `Cert`: machine-checkable evidence for the obligations below

If any obligation fails, compilation MUST stop with an error.

#### 9.2.2 Obligations (must hold)

- `L1 Type consistency`: every edge in `G_lowered` has identical source/target endpoint types.
- `L2 Widening safety`: inserted conversion nodes are only from the allowed chains
  `int8 -> int16 -> int32 -> float -> double` and `cfloat -> cdouble`.
- `L3 Rate/shape preservation`: inserted widening nodes are 1:1 and MUST NOT alter
  token rate or shape constraints.
- `L4 Monomorphization soundness`: each polymorphic call is rewritten to exactly one
  concrete actor instance selected by solved type substitution.
- `L5 No fallback typing`: unresolved or ambiguous types MUST be diagnostics; silent
  fallback types are forbidden.

#### 9.2.3 Backend contract

`codegen` MUST consume `TypedScheduledIR` and MUST NOT:

- re-run type inference
- infer fallback wire types
- reinterpret unresolved parameter types

`codegen` is a syntax-directed serialization from IR to C++.

---

## 10. Error Output Format

All diagnostics are written to stderr in the following format:

```
<level>: <message>
  at <file>:<line>:<column>
  <context line>
  <caret indicator>
  hint: <suggestion>
```

### 10.1 Levels

| Level | Meaning |
|-------|---------|
| `error` | Compilation cannot continue. Exit code 1. |
| `warning` | Potential issue, compilation continues. |
| `info` | Additional context attached to a preceding error or warning. |

### 10.2 Error categories

Errors are classified by phase (see lang spec §7.1 for the full catalog):

| Category | Example |
|----------|---------|
| Syntax | Unexpected token, missing `}` |
| Name resolution | Unknown actor, undefined buffer |
| Type mismatch | Pipe endpoint types differ |
| Type inference | Ambiguous polymorphic call, unsatisfied type constraints |
| Lowering verification | Obligation failure in typed lowering (L1-L5) |
| Rate mismatch | Cross-clock `Pw × fw ≠ Cr × fr` |
| SDF | Balance equation unsolvable, missing delay |
| Constraint | Multiple writers, unused tap |
| Memory | Buffer total exceeds `set mem` |
| Narrowing (warning) | Explicit narrowing conversion may lose precision |
| Actor | Duplicate definition, missing registration |

### 10.3 Example

```
error: type mismatch at pipe 'fft -> fir'
  at example.pdl:12:25
    adc(0) | fft(256) | fir(coeff) -> signal
                        ^^^^^^^^^^
  fft outputs cfloat[256], but fir expects float[5]
  hint: insert an explicit conversion actor (e.g. c2r)
```

```text
error: ambiguous polymorphic actor call 'fir(...)'
  at example.pdl:18:21
    constant(0.0) | fir(coeff) | stdout()
                    ^^^^^^^^^^
  candidates: fir<float>, fir<double>
  hint: specify type arguments explicitly, e.g. fir<float>(coeff)
```

```text
error: lowering verification failed (L3 rate/shape preservation)
  inserted widening node changed token rate at edge 'a -> b'
  hint: report as compiler bug; no code was generated
```

---

## 11. Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Compilation succeeded |
| `1` | Compilation failed (source errors) |
| `2` | Usage error (invalid flags, missing input file) |
| `3` | System error (C++ compiler not found, write permission denied) |

---

## 12. Generated Code Structure

When `--emit cpp` is used, the generated file contains:

1. **Includes**: `<pipit.h>`, actor headers, standard library headers
2. **Monomorphized actor aliases** (if polymorphism used): concrete typed actor instances
3. **Static storage**: `const` arrays, ring buffer allocations, parameter initial values
4. **Task functions**: One function per task containing the schedule loop
5. **Mode dispatch** (if CSDF): Control subgraph execution + mode switch logic
6. **Double-buffer swap points**: Runtime parameter updates at iteration boundaries
7. **`main()`**: CLI argument parser (`--duration`, `--threads`, `--param`, `--probe`, `--stats`), thread launch, signal handler, graceful shutdown

Code generation MUST be fully driven by `TypedScheduledIR` emitted from phase 8.
Type fallback at this stage is non-conforming.

The generated code depends only on:

- `libpipit` (runtime library)
- Actor headers for generated C++ declarations (`-I` / `--include` / `--actor-path`)
- Optional actor metadata manifest (`--actor-meta`) for registry loading
- C++17 standard library

---

## 13. Performance / Safety

- Compilation of a 100-actor graph should complete in under 1 second on commodity hardware
- Memory usage scales linearly with graph size; no unbounded allocations during compilation
- With explicit `--actor-meta`, `pcc` only reads metadata and does not execute user-provided code
- Without `--actor-meta`, `pcc` scans trusted headers from `-I`/`--include`/`--actor-path`
- Generated code inherits safety properties from `libpipit` (bounded buffers, no dynamic allocation at runtime)

---

## 14. Acceptance Tests

```bash
# 1. Compile and produce executable
pcc example.pdl --actor-meta build/actors.meta.json -I actors.h -o example
test -x ./example

# 2. Emit C++ source only
pcc example.pdl --actor-meta build/actors.meta.json -I actors.h --emit cpp -o example_gen.cpp
test -f ./example_gen.cpp

# 3. AST dump completes without error
pcc example.pdl --emit ast

# 4. Graph dump shows repetition vectors and buffer sizes
pcc example.pdl -I actors.h --emit graph | grep "repetition_vector"
pcc example.pdl -I actors.h --emit graph | grep "buffer_size"

# 5. graph-dot / schedule / timing-chart emits complete
pcc example.pdl -I actors.h --emit graph-dot | grep "digraph"
pcc example.pdl -I actors.h --emit schedule | grep "task"
pcc example.pdl -I actors.h --emit timing-chart | grep "gantt"

# 6. Type mismatch produces actionable error
echo 'clock 1kHz t { adc(0) | fir(coeff) -> out }' > bad.pdl
pcc bad.pdl -I actors.h 2>&1 | grep "error: type mismatch"

# 7. Ambiguous polymorphic call requests explicit type args
echo 'const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]
clock 1kHz t { constant(0.0) | fir(coeff) | stdout() }' > ambiguous.pdl
pcc ambiguous.pdl -I actors.h 2>&1 | grep "error: ambiguous polymorphic actor call"

# 8. Lowering verification failures abort compilation
# (compiler-internal check; expected only in negative compiler tests)
pcc lowering_bug_case.pdl -I actors.h 2>&1 | grep "error: lowering verification failed"

# 9. Missing actor header produces name resolution error
pcc example.pdl 2>&1 | grep "error:"

# 10. Release build strips probes
pcc example.pdl -I actors.h --release -o example_rel
# (verify no probe symbols in binary)

# 11. Version and help flags
pcc --version
pcc --help

# 12. CSDF example compiles
pcc receiver.pdl -I actors.h -o receiver
test -x ./receiver

# 13. Invalid flag returns exit code 2
pcc --invalid-flag; test $? -eq 2
```
