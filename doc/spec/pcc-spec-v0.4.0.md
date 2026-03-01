# Feature: pcc — Pipit Compiler Collection

Version: 0.4.0

## 1. Goal

`pcc` is the compiler for Pipit Definition Language (`.pdl`) files.
It consumes pipeline source plus actor metadata inputs, performs static checks, and emits one of:

- generated C++ source
- a compiled executable
- intermediate/debug artifacts (`ast`, `graph`, `schedule`, etc.)
- deterministic metadata/provenance artifacts (`manifest`, `build-info`)

v0.4.0 keeps the v0.3 language surface, but tightens compiler contracts:

- explicit IR boundaries (`AST -> HIR -> THIR -> LIR`)
- dependency-driven pass execution per `--emit` target
- unified diagnostics model (human and JSON format)
- generated runtime shell delegation via `pipit_shell.h`
- deterministic registry fingerprint and build provenance output

Refer to [pipit-lang-spec-v0.4.0](pipit-lang-spec-v0.4.0.md) for language semantics.
This document defines `pcc` behavior as a compiler tool.

## 2. Non-goals

- `pcc` does not parse C++ actor implementation bodies.
- `pcc` does not provide mandatory incremental/watch mode.
- `pcc` does not include distributed build cache in v0.4.0.
- `pcc` does not manage external C++ dependencies beyond `libpipit`.
- No protocol-level reliability guarantees beyond each transport spec (`PPKT` / `PSHM`).
- IDE/LSP behavior is out of scope for this spec.

---

## 3. Compatibility Gate

v0.4.0 adopts a compatibility gate:

- Default behavior keeps v0.3.x language and CLI compatibility unless v0.4.0 language spec deltas are explicitly enabled.
- Any breaking behavior requires all of:
  - explicit spec delta in this file (or successor spec),
  - dedicated ADR with migration reasoning,
  - release-note entry with impact and migration path.

Compatibility gate scope includes:

- language parsing/typing behavior,
- `pcc` CLI options and defaults,
- output-stage semantics (`--emit`),
- runtime option behavior in generated binaries (`--duration`, `--param`, `--bind`, `--probe`, `--probe-output`, `--stats`).

### 3.1 Implementation

`pcc` is implemented in Rust (see [ADR-001](../adr/001-rust-for-pcc.md)).
Generated C++ targets C++20 and links `libpipit`.

There is no Rust/C++ FFI boundary inside compiler phases. `pcc`:

- reads metadata from manifest or header scanning
- builds compiler IR artifacts in Rust
- emits C++ text
- optionally invokes system C++ compiler as subprocess for `--emit exe`

---

## 4. Software Architecture

```text
                         ┌─────────────────────────────────────────────┐
                         │              pcc (Rust binary)              │
                         │                                             │
  ┌──────────┐           │  ┌─────────┐   ┌─────────────┐             │
  │ .pdl     │──────────▶│  │  Lexer  │──▶│  Parser     │──┐          │
  │ source   │           │  └─────────┘   └─────────────┘  │ AST      │
  └──────────┘           │                                  ▼          │
                         │                         ┌─────────────┐     │
  ┌─────────────┐        │  ┌───────────────┐      │   Resolve   │     │
  │ actors.meta │───────▶│  │ Actor Registry │────▶└─────────────┘     │
  │ / headers   │        │  └───────────────┘            │             │
  └─────────────┘        │                               ▼             │
                         │                         ┌─────────────┐     │
                         │                         │  Build HIR   │     │
                         │                         └─────────────┘     │
                         │                               │             │
                         │                               ▼             │
                         │                      ┌─────────────────┐    │
                         │                      │ Type + Lowering │    │
                         │                      │  (THIR + Cert)  │    │
                         │                      └─────────────────┘    │
                         │                               │             │
                         │                               ▼             │
                         │                      ┌─────────────────┐    │
                         │                      │ Graph + Analyze │    │
                         │                      └─────────────────┘    │
                         │                               │             │
                         │                               ▼             │
                         │                      ┌─────────────────┐    │
                         │                      │    Schedule     │    │
                         │                      └─────────────────┘    │
                         │                               │             │
                         │                               ▼             │
                         │                      ┌─────────────────┐    │
                         │                      │    Build LIR    │    │
                         │                      └─────────────────┘    │
                         │                               │             │
                         │                               ▼             │
                         │                      ┌─────────────────┐    │
                         │                      │     Codegen     │────┼──▶ *_gen.cpp
                         │                      └─────────────────┘    │
                         └───────────────────────────────┬─────────────┘
                                                         │ [emit exe]
                                                         ▼
                                      System C++ compiler (--cc) + libpipit
```

### 4.1 Module mapping (Rust crate: `pcc`)

| Pass | Input IR | Output IR/Artifact | Owns |
|---|---|---|---|
| Parse | source text | `AST` | grammar/lexing |
| Resolve + Normalize | `AST`, registry | `HIR` | symbols, scope, normalization |
| Type Infer + Mono + Lower Verify | `HIR`, registry | `THIR` | typing, monomorphization, widening safety |
| Graph/Analyze/Schedule | `THIR` | `LIR` | graph facts, rates, buffers, schedule |
| Bind Infer + Contract Check | `LIR` | `BindInterface` | bind direction/contract inference, stable-id assignment |
| Codegen | `LIR`, `BindInterface` | C++ source | serialization only |

Module detail:

| Module | Responsibility |
|--------|---------------|
| `lexer` | Tokenize `.pdl` source |
| `parser` | Build AST from token stream |
| `registry` | Load actor metadata from manifest/header scan |
| `resolve` | Name resolution (actors, params, buffers, taps) |
| `hir` | Build normalized semantic IR (define/mode/task normalization) |
| `types` | Type inference and monomorphization |
| `lower` | Lower typed graph and emit lowering certificate |
| `graph` | SDF graph construction |
| `analyze` | Static analysis (types, rates, buffers, constraints) |
| `schedule` | PASS schedule and K-factor generation |
| `lir` | Backend-ready IR construction |
| `codegen` | C++ serialization from LIR only |
| `pass` / `pipeline` | Pass dependency graph and phase orchestration |
| `diag` | Unified diagnostics model and formatting |
| `provenance` | Source/registry fingerprinting, build-info output |

### 4.2 Data flow between modules

```text
.pdl + actor metadata
  -> Parse (AST)
  -> Resolve
  -> HIR
  -> Type inference + monomorphization
  -> Lowering + verification (THIR + Cert)
  -> Graph construction
  -> Static analysis
  -> Schedule generation
  -> LIR construction
  -> C++ code generation
```

### 4.3 Pass Manager Contract

Each pass declares:

- `inputs`: required artifacts and config fields,
- `outputs`: produced artifacts,
- `invariants`: pre/post conditions,
- `invalidation_key`: deterministic hash inputs used for cache validity.

`--emit` targets resolve required artifacts via dependency graph and evaluate the minimal pass subset.

### 4.4 Artifact/Caching Contract

- Artifact keys are deterministic across machines for equal inputs/config.
- Registry provenance (manifest/header hash set, schema version) participates in invalidation.
- Cache miss or verification failure falls back to recompute.
- Cache behavior must not change observable compiler semantics.
- `BindInterface` (backing optional interface manifest output) is a first-class artifact and participates in deterministic invalidation.

---

## 5. Inputs

### 5.1 Source file (`.pdl`)

Primary input for compile stages:

- `.pdl` source (required),
- actor metadata:
  - preferred: `--actor-meta` manifest (`actors.meta.json`),
  - fallback: header scanning via `-I` / `--include` / `--actor-path`,
- compilation config (`--emit`, `--cc`, `--cflags`, `--release`, etc.),
- bind endpoint overrides (`--bind <name>=<endpoint>`, optional, repeatable),
- optional interface manifest output path (`--interface-out <path>`).

```bash
pcc example.pdl [OPTIONS]
```

Emit stages:

- `--emit ast`: AST dump,
- `--emit graph`: analysis graph dump,
- `--emit graph-dot`: DOT graph,
- `--emit schedule`: schedule dump,
- `--emit timing-chart`: Mermaid timing chart,
- `--emit cpp`: generated C++,
- `--emit interface` (optional): bind contract manifest (`stable_id`, direction, contract, endpoint),
- default `--emit exe`: executable via system C++ compiler.

Exceptions:

- `--emit manifest` may run without `.pdl`
- `--emit build-info` requires source text but does not require parse success

### 5.2 Actor metadata manifest (`--actor-meta`)

JSON metadata manifest (schema v1) consumed by actor registry:

```bash
pcc example.pdl --actor-meta ./build/actors.meta.json
```

**Required** for all stages that need actor metadata (`cpp`, `exe`, `build-info`, `graph`, `graph-dot`, `schedule`, `timing-chart`). Omitting `--actor-meta` on these stages produces E0700 (exit code 2).

Not required for `--emit manifest` (which generates the manifest) or `--emit ast` (parse-only dump).

### 5.3 Actor headers (`-I`, `--include`) — manifest generation and C++ declaration input

One or more header files or directories:

```bash
pcc example.pdl -I ./actors.h -I ./include/
```

For `--emit manifest`, headers provide the source for metadata extraction (via preprocessor-based scanning).
For `--emit exe` and `--emit cpp`, headers are used as generated C++ declaration inputs (not for metadata — that comes from `--actor-meta`).

### 5.4 Actor search path (`--actor-path`)

Directories recursively scanned for actor headers:

```bash
pcc example.pdl --actor-path ./actors --actor-path ./vendor/actors
```

Name conflict rule: `-I` entries have precedence over `--actor-path`.

### 5.5 Bind Compilation Contract

For the `bind` specification (`pipit-lang-spec-v0.4.0`, section 5.11), `pcc` must satisfy the following.

1. **direction inference**
   - If `-> name` exists, infer `out`.
   - If `-> name` does not exist and only `@name` exists, infer `in`.
   - Otherwise, emit a compile-time error.

1. **contract inference**
   - `dtype` / `shape`: determined from buffer type information in LIR.
   - `rate_hz`: determined from `tokens_per_iter * task_rate_hz` on writer/reader sides.
   - For an `in` bind, if required rates from multiple readers do not match, emit an error.

1. **stable_id assignment**
   - `stable_id` is generated deterministically from semantic IDs (task/node/edge lineage), not span/name text.
   - It must remain stable for identical input and compiler configuration (deterministic).

1. **endpoint validation**
   - Validate `udp` / `unix_dgram` endpoint arguments against the PPKT spec.
   - Validate `shm` endpoint arguments against the PSHM spec.

1. **manifest emission**
   - Emit an interface manifest when `--emit interface` or `--interface-out <path>` is specified.
   - When emitted, the manifest must contain bind contract information consistent with generated C++.

`pcc` MUST NOT change the SDF schedule as a side effect of bind inference/validation.

---

## 6. Outputs

### 6.1 Default: Executable (`--emit exe`)

By default, `pcc` generates C++, invokes `--cc`, and links runtime:

```bash
pcc example.pdl -I actors.h -o receiver
# produces: ./receiver
```

### 6.2 `--emit cpp`: Generated C++ source only

```bash
pcc example.pdl -I actors.h --emit cpp -o receiver_gen.cpp
```

v0.4.0 destination contract:

- with `-o`: write file
- without `-o`: write to stdout

### 6.3 `--emit ast`: AST dump

```bash
pcc example.pdl --emit ast
```

### 6.4 `--emit graph`: analysis graph dump

```bash
pcc example.pdl -I actors.h --emit graph
```

### 6.5 `--emit graph-dot`: Graphviz DOT dump

```bash
pcc example.pdl -I actors.h --emit graph-dot
```

### 6.6 `--emit schedule`: schedule dump

```bash
pcc example.pdl -I actors.h --emit schedule
```

### 6.7 `--emit timing-chart`: Mermaid Gantt dump

```bash
pcc example.pdl -I actors.h --emit timing-chart
```

### 6.8 `--emit manifest`: canonical actor metadata

Emits canonical `actors.meta.json` derived from scanned headers:

```bash
pcc --emit manifest -I actors.h -o actors.meta.json
```

`--emit manifest` is a usage error when combined with `--actor-meta`.

### 6.9 `--emit build-info`: provenance JSON

Outputs machine-readable provenance:

```bash
pcc example.pdl -I actors.h --emit build-info
```

Fields:

- `source_hash` (sha256 of source text)
- `registry_fingerprint` (sha256 of canonical registry JSON)
- `manifest_schema_version`
- `compiler_version`

### 6.10 Diagnostics

- Human-readable diagnostics remain default CLI output.
- Machine-readable mode (`json`) provides structured diagnostics for tooling.
- Diagnostic stability policy: adding codes is allowed; changing meaning of existing codes requires versioned note.
- Bind-related diagnostics include at least:
  - direction inference failure,
  - contract ambiguity/mismatch,
  - duplicate bind target,
  - unsupported endpoint option/value.

---

## 7. CLI Interface

```text
pcc [source.pdl] [OPTIONS]
```

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-o <path>` | PATH | stage-dependent | Output path |
| `--actor-meta <file>` | PATH | — | Actor metadata manifest |
| `-I, --include <path>` | PATH (repeatable) | — | Actor header or search path |
| `--actor-path <dir>` | PATH (repeatable) | — | Recursive actor search directory |
| `--emit <stage>` | enum | `exe` | `exe`, `cpp`, `ast`, `graph`, `graph-dot`, `schedule`, `timing-chart`, `manifest`, `build-info`, `interface` |
| `--release` | flag | off | Release codegen profile |
| `--cc <compiler>` | STRING | `clang++` | System C++ compiler command |
| `--cflags <flags>` | STRING | mode-dependent | Additional C++ compiler flags |
| `--bind <name>=<endpoint>` | STRING (repeatable) | — | Bind endpoint override |
| `--interface-out <path>` | PATH | — | Interface manifest output path |
| `--diagnostic-format <fmt>` | enum | `human` | `human` or `json` |
| `--verbose` | flag | off | Phase timing and pass trace |
| `--version` | flag | — | Print version and exit |
| `--help` | flag | — | Print help and exit |

### 7.1 Compilation failure conditions

Compilation fails (exit code 1) when any of the following occur:

- parsing fails,
- resolution fails,
- type/lowering verification fails,
- analysis/scheduling invariants fail,
- bind inference/contract validation fails,
- backend emission prerequisites are missing,
- external C++ compilation fails for `--emit exe`.

### 7.2 Release vs Debug build

| Behavior | Debug (default) | Release (`--release`) |
|----------|------------------|----------------------|
| Probe instrumentation | Included | Stripped |
| C++ optimization | `-O0 -g` | `-O2` |
| C++ standard | `-std=c++17` | `-std=c++17` |
| Runtime assertions | Enabled | Reduced |

When `--cflags` is explicitly set, optimization defaults are overridden.

---

## 8. Actor Metadata Loading

### 8.1 Compilation stages (`cpp`, `exe`, `build-info`, `graph`, `graph-dot`, `schedule`, `timing-chart`)

`--actor-meta` is required. Loading order:

1. Load manifest metadata from `--actor-meta`.
2. Validate schema and required fields.
3. Build registry keyed by actor name and type parameters.

Missing `--actor-meta` produces E0700 (exit code 2).

### 8.2 Manifest generation (`--emit manifest`)

`--actor-meta` is NOT used. Metadata is extracted from headers:

1. Discover actor headers from `-I` and `--actor-path`.
2. Build probe translation unit with redefined `ACTOR` macro (ADR-032).
3. Invoke preprocessor (`<cc> -E -P -x c++ -std=c++20 -`) to resolve includes/conditionals.
4. Parse `PIPIT_REC_V1(...)` records in Rust.
5. Apply overlay precedence: `--actor-path` is base, `-I` overlays with higher precedence on name collision. Duplicates within the same source class are errors.
6. Emit canonical schema v1 JSON.

### 8.3 Parse-only dump (`--emit ast`)

No actor metadata required. Registry is not loaded.

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
      "out_type": { "TypeParam": "T" },
      "out_count": { "Literal": 1 },
      "params": [
        { "kind": "Param", "param_type": { "SpanTypeParam": "T" }, "name": "coeff" },
        { "kind": "Param", "param_type": "Int", "name": "N" }
      ]
    }
  ]
}
```

Registry determinism requirements:

- canonical actor ordering for manifest output
- compact canonical JSON for fingerprint hashing
- same inputs must produce same fingerprint across platforms

Typical errors:

```text
error: invalid actor metadata schema (expected: 1, found: 0)
error: duplicate actor 'fft' in metadata registry
error: --emit manifest cannot be used with --actor-meta
```

---

## 9. Compilation Phases

Compilation is pass-based and dependency-driven.
A phase failure aborts compilation.

```text
source + registry inputs
  │
  ├─ 1. Lex & Parse                          [--emit ast]
  │     └─ Build AST
  │
  ├─ 2. Actor Loading
  │     └─ Build actor registry
  │
  ├─ 2.5 Spawn Expansion (§5.4.5)
  │     └─ Expand clock[idx=begin..end] into N independent tasks
  │
  ├─ 3. Name Resolution
  │     └─ Bind actors/params/consts/buffers/taps
  │
  ├─ 4. HIR Construction
  │     └─ Normalize defines/modes/tasks into HIR
  │
  ├─ 5. Type Inference & Monomorphization
  │     └─ Solve constraints, instantiate concrete actors
  │
  ├─ 6. Typed Lowering + Verification
  │     └─ Create THIR and validate lowering obligations
  │
  ├─ 7. Graph + Static Analysis              [--emit graph, --emit graph-dot]
  │     └─ Rates, balance, delays, buffers, constraints
  │
  ├─ 8. Schedule Generation                  [--emit schedule, --emit timing-chart]
  │     └─ PASS order, K-factor, fusion planning
  │
  ├─ 9. LIR Build + C++ Codegen              [--emit cpp]
  │     └─ Backend-ready IR then C++ serialization
  │
  └─ 10. C++ Compilation                      [--emit exe]
        └─ Invoke system compiler and link runtime
```

| Phase | Description | Can emit here |
|-------|-------------|---------------|
| 1. Lex & Parse | Tokenize and build AST | `--emit ast` |
| 2. Actor Loading | Load metadata registry | |
| 2.5 Spawn Expansion | Expand spawn clauses into N tasks | |
| 3. Name Resolution | Resolve symbols and references | |
| 4. HIR Construction | Normalize semantic graph | |
| 5. Type Inference & Monomorphization | Solve types and instantiate actors | |
| 6. Typed Lowering + Verification | Build THIR and verify obligations | |
| 7. Graph + Static Analysis | Build graph and solve rate/buffer constraints | `--emit graph`, `--emit graph-dot` |
| 8. Schedule Generation | Build execution schedule | `--emit schedule`, `--emit timing-chart` |
| 9. LIR Build + C++ Codegen | Emit C++ from LIR | `--emit cpp` |
| 10. C++ Compilation | Compile and link executable | `--emit exe` |

Spec-level acceptance criteria:

- `AST -> HIR -> THIR -> LIR` boundary and ownership are documented and internally consistent.
- Pass-manager contract (inputs/outputs/invalidation/invariants) is documented.
- Compatibility gate is documented and explicit.
- Bind inference contract is documented (`direction`, `dtype/shape/rate`, deterministic `stable_id`).
- Optional interface manifest artifact contract is documented.
- ADR set for Phase 0 architecture decisions is published:
  - `ADR-020` pass-manager artifact model,
  - `ADR-021` stable semantic IDs,
  - `ADR-022` diagnostics model,
  - `ADR-023` backward-compatibility gate.

### 9.1 Rate-domain (fusion-domain) optimization

`pcc` may fuse adjacent firings to reduce loop overhead when all are true:

- same task and same subgraph
- adjacent in topological order
- compatible repetition/rate transfer
- no delay/back-edge barrier crossing
- no mandatory barrier node between firings

Fusion is optional. Unfused schedules remain conforming.

### 9.2 Typed IR and Verified Lowering (Normative)

Lowering contract:

```text
Lower(TypedHIR) -> (THIR, Cert)
```

Required obligations:

- `L1` Type consistency on every THIR edge
- `L2` Widening safety (only allowed widening chains)
- `L3` Rate/shape preservation by inserted conversions
- `L4` Monomorphization soundness (one resolved concrete target)
- `L5` No fallback typing on unresolved types

Backend contract:

- codegen consumes LIR (derived from THIR/analysis/schedule artifacts)
- codegen must not re-run type inference
- codegen must not invent fallback types

---

## 10. Error Output Format

Default output is human-readable diagnostics to stderr:

```text
<level>[<code>]: <message>
  at <file>:<line>:<column>
  <context line>
  <caret>
  hint: <suggestion>
```

JSON format (`--diagnostic-format json`) emits one JSON object per line with:

- `code`
- `level`
- `message`
- `primary_span`
- `related_spans` (optional)
- `hint` (optional)

### 10.1 Levels

| Level | Meaning |
|-------|---------|
| `error` | Compilation stops (exit code 1) |
| `warning` | Compilation may continue |
| `info` | Supplementary context |

### 10.2 Error categories

| Category | Example |
|----------|---------|
| Syntax | Unexpected token |
| Name resolution | Unknown actor / symbol |
| Type mismatch | Pipe endpoint type incompatibility |
| Type inference | Ambiguous polymorphic call |
| Lowering verification | L1-L5 obligation failure |
| Rate/SDF | Unsatisfied balance equations |
| Constraint | Invalid writer/reader ownership |
| Usage | Incompatible CLI flags |
| System | I/O failure, tool invocation failure |

### 10.3 Example

```text
error[E0201]: type mismatch at pipe 'fft -> fir'
  at example.pdl:12:25
    adc(0) | fft(256) | fir(coeff) -> signal
                        ^^^^^^^^^^
  hint: insert an explicit conversion actor
```

```text
error[E0601]: lowering verification failed (L3 rate/shape preservation)
  hint: this indicates invalid lowered IR and codegen was skipped
```

### 10.4 Diagnostic code compatibility policy

- Reuse prohibition: once assigned, a diagnostic code must never be reassigned to a different meaning.
- Retirement rule: removed diagnostics retire their code permanently.
- Semantics change rule: when meaning changes, allocate a new code and deprecate the old one.
- Test contract: tests may assert on `Diagnostic.code`; semantic changes to existing codes are breaking changes.
- Versioning: code meanings are versioned with the compiler version.

### 10.5 Code ranges

| Range | Phase | Description |
|-------|-------|-------------|
| E0001-E0099 | resolve | Name resolution errors |
| E0100-E0199 | type_infer | Type inference errors |
| E0200-E0299 | lower | Lowering verification (L1-L5) |
| E0300-E0399 | analyze | SDF analysis errors |
| E0400-E0499 | schedule | Scheduling errors |
| E0500-E0599 | graph | Graph construction errors |
| E0600-E0699 | pipeline | Stage certification failures |
| E0700-E0799 | usage | CLI usage errors |
| W0001-W0099 | resolve | Name resolution warnings |
| W0300-W0399 | analyze | SDF analysis warnings |
| W0400-W0499 | schedule | Scheduling warnings |

### 10.6 Assigned diagnostic codes

#### 10.6.1 Resolve (E0001-E0035, W0001-W0002)

| Code | Description |
|------|-------------|
| E0001 | Duplicate const definition |
| E0002 | Duplicate param definition |
| E0003 | Duplicate define definition |
| E0004 | Duplicate task definition |
| E0005 | Cross-namespace name collision |
| E0006 | Tap declared but never consumed in define |
| E0007 | Duplicate mode in task |
| E0008 | Undefined tap reference |
| E0009 | Duplicate tap declaration |
| E0010 | Multiple writers to shared buffer |
| E0011 | Unknown actor or define |
| E0012 | Non-polymorphic actor called with type arguments |
| E0013 | Wrong number of type arguments |
| E0014 | Undefined param reference |
| E0015 | Undefined const reference |
| E0016 | Runtime param used as frame dimension |
| E0017 | Unknown name in shape constraint |
| E0018 | Undefined param in switch source |
| E0019 | Switch references undefined mode |
| E0020 | Mode defined but not listed in switch |
| E0021 | Mode listed multiple times in switch |
| E0022 | Undefined tap as actor input |
| E0023 | Shared buffer has no writer |
| E0024 | Duplicate bind definition |
| E0025 | Bind target not referenced (reserved) |
| E0026 | Spawn range invalid (begin >= end) |
| E0027 | Spawn bound not a compile-time integer |
| E0028 | Shared array size not a positive integer |
| E0029 | Unknown const in spawn range |
| E0030 | Unknown const in shared size |
| E0031 | Shared array index out of bounds |
| E0032 | Buffer subscript on non-array buffer |
| E0033 | Star-writer conflicts with element-writer |
| E0034 | Duplicate shared array name |
| E0035 | Buffer index const is not a non-negative integer |
| W0001 | Define shadows actor with same name |
| W0002 | Deprecated switch default clause |

#### 10.6.2 Type inference (E0100-E0102)

| Code | Description |
|------|-------------|
| E0100 | Unknown type name |
| E0101 | Ambiguous polymorphic call (upstream context available) |
| E0102 | Ambiguous polymorphic call (no upstream context) |

#### 10.6.3 Lowering (E0200-E0206)

| Code | Description |
|------|-------------|
| E0200 | L1: Type consistency violation at edge |
| E0201 | L2: Unsafe widening chain |
| E0202 | L3: Rate/shape preservation violation |
| E0203 | L4: Polymorphic actor not fully monomorphized |
| E0204 | L4: Polymorphic actor has no concrete instance |
| E0205 | L5: Unresolved input type |
| E0206 | L5: Unresolved output type |

#### 10.6.4 Analysis (E0300-E0312, W0300)

| Code | Description |
|------|-------------|
| E0300 | Unresolved frame dimension |
| E0301 | Conflicting frame constraint from upstream |
| E0302 | Conflicting dimension (span-derived vs edge-inferred) |
| E0303 | Type mismatch at pipe |
| E0304 | SDF balance equation unsolvable |
| E0305 | Feedback loop with no delay |
| E0306 | Shared buffer rate mismatch |
| E0307 | Shared memory pool exceeded |
| E0308 | Param type mismatch |
| E0309 | Switch param non-int32 default |
| E0310 | Control buffer type mismatch |
| E0311 | Bind target not referenced in any task |
| E0312 | Bind contract conflict (readers disagree on type/shape/rate) |
| W0300 | Inferred dimension param ordering warning |

#### 10.6.5 Schedule (E0400, W0400)

| Code | Description |
|------|-------------|
| E0400 | Unresolvable cycle in subgraph |
| W0400 | Unsustainable tick rate |

#### 10.6.6 Graph (E0500)

| Code | Description |
|------|-------------|
| E0500 | Tap not found in graph |

#### 10.6.7 Pipeline certification (E0600-E0603)

| Code | Description |
|------|-------------|
| E0600 | HIR verification failed (H1-H3) |
| E0601 | Lowering verification failed (L1-L5) |
| E0602 | Schedule verification failed (S1-S2) |
| E0603 | LIR verification failed (R1-R2) |

#### 10.6.8 Usage (E0700)

| Code | Description |
|------|-------------|
| E0700 | `--actor-meta` is required for the requested `--emit` stage |

E0700 respects `--diagnostic-format`:

- **Human**: `error[E0700]: --actor-meta is required for --emit <stage>` with hint showing manifest generation and compile commands.
- **JSON**: `{"kind":"usage","level":"error","code":"E0700",...}` with `"usage"` kind.

---

## 11. Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Compilation failure (source/semantic errors) |
| `2` | Usage failure (invalid or incompatible options) |
| `3` | System failure (I/O, missing tools, permission) |

---

## 12. Generated Code Structure

For `--emit cpp`, generated code is structured as:

1. Includes (`pipit.h`, `pipit_shell.h`, actor headers, standard headers)
2. Concrete actor aliases and static metadata tables
3. Static storage (buffers, const data, params)
4. Task functions (scheduled actor firing logic)
5. Mode/control dispatch (when applicable)
6. Runtime handoff (`pipit::shell_main(desc)`)

v0.4.0 contract:

- generated runtime shell behavior is centralized in `pipit_shell.h`
- generated code contains task logic, not ad-hoc CLI parser duplication
- code generation is deterministic for identical inputs

Dependencies:

- `libpipit`
- actor headers (`-I` / `--actor-path`)
- C++20 toolchain

---

## 13. Performance / Safety

- Pass execution is dependency-driven; irrelevant phases are skipped for non-terminal artifacts.
- Compilation memory growth should be linear to graph size.
- Metadata-only workflows (`--emit manifest`) avoid `.pdl` parsing.
- Build provenance (`--emit build-info`) is machine-readable and deterministic.
- Generated runtime keeps bounded-buffer semantics inherited from `libpipit`.

---

## 14. Acceptance Tests

```bash
# 1. Build executable
pcc example.pdl -I actors.h -o example
test -x ./example

# 2. Emit C++ to file
pcc example.pdl -I actors.h --emit cpp -o example_gen.cpp
test -f ./example_gen.cpp

# 3. Emit C++ to stdout when -o is omitted
pcc example.pdl -I actors.h --emit cpp | head -n 1 | grep "pcc"

# 4. Emit AST
pcc example.pdl --emit ast >/dev/null

# 5. Emit graph artifacts
pcc example.pdl -I actors.h --emit graph | grep "repetition"
pcc example.pdl -I actors.h --emit graph-dot | grep "digraph"

# 6. Emit schedule artifacts
pcc example.pdl -I actors.h --emit schedule | grep "task"
pcc example.pdl -I actors.h --emit timing-chart | grep "gantt"

# 7. Emit manifest without source file
pcc --emit manifest -I actors.h -o actors.meta.json
test -f actors.meta.json

# 8. Emit build-info
pcc example.pdl -I actors.h --emit build-info | grep "source_hash"

# 9. JSON diagnostic mode
pcc bad.pdl --diagnostic-format json 2>&1 | head -n 1 | grep "\"code\""

# 10. Invalid flag returns usage error
pcc --invalid-flag
test $? -eq 2
```
