# ADR-026: Runtime Shell Library

## Context

Each compiled Pipit pipeline generates a complete, self-contained C++ runtime shell: CLI argument parsing (`--param`, `--duration`, `--threads`, `--probe`, `--probe-output`, `--stats`), probe initialization, SIGINT handling, thread lifecycle, duration wait, and statistics output. This shell constitutes 30-40% of generated code volume (~120-150 LOC of `main()` per pipeline) and is 99% generic boilerplate — varying only by parameter names, task names, probe names, and buffer names.

After Phase 2b (ADR-025) established the LIR as the sole codegen input, the opportunity to extract this boilerplate became clear: all pipeline-varying data is already pre-resolved in `LirProgram`.

## Decision

Extract generic runtime shell logic into a header-only `pipit_shell.h` library. Codegen emits compact **descriptor tables** (program-specific data) and a single `pipit::shell_main()` call instead of emitting the full shell inline.

### Design: Descriptor Table + Shell Function

**Descriptor types** (in `pipit_shell.h`):

- `ParamDesc`: `{name, apply}` — name string + function pointer to parse-and-store
- `TaskDesc`: `{name, entry, stats}` — name + task function pointer + stats accumulator
- `BufferStatsDesc`: `{name, available, elem_size}` — for stats output
- `ProbeDesc`: `{name, enabled}` — name + pointer to per-probe enable flag
- `RuntimeState`: pointers to generated globals (`stop`, `exit_code`, `start`, `stats`, `probe_output`)
- `ProgramDesc`: aggregates all descriptors + policy/memory info

**`shell_main(argc, argv, desc)`** implements:

- CLI parsing, probe initialization, SIGINT handler, thread launch/join, duration wait, stats output

**What stays in generated code** (unchanged):

- All global state: atomics, parameters, ring buffers, constants, stats, probe flags
- All task function bodies (pipeline-specific actor wiring)
- Descriptor table definitions (compact data referencing the globals above)

### Probe initialization gate

Probe initialization uses `desc.probes.empty()` as the sole gate. `#ifndef NDEBUG` is not used in the shell. This is an intentional simplification: release codegen guarantees `probes` is empty, so the single gate is sufficient and easier to reason about.

**Behavior change**: In the edge case of debug codegen compiled with `-DNDEBUG`, unknown probe names now produce an error (previously silently accepted). This is considered a correctness improvement.

### `_probe_output_file` always emitted

The `static FILE* _probe_output_file = nullptr;` global is always generated, even in release mode. This ensures the symbol exists for `RuntimeState::probe_output`, which always points to it. In release mode, task functions' probe output code (guarded by `#ifndef NDEBUG`) references this symbol — the C++ compiler strips the dead code but the symbol must exist for compilation.

## Consequences

- **Generated code reduction**: ~90-120 LOC saved per pipeline (`main()` shrinks from ~150 LOC to ~25 LOC of descriptor tables)
- **Codegen simplification**: ~180 LOC net reduction in `codegen.rs` (4 emit methods removed)
- **Preamble simplification**: 13 includes → 3 (`pipit.h`, `pipit_shell.h`, `cstdio`)
- **Maintenance**: Shell behavior changes (e.g., new CLI flags) are made once in `pipit_shell.h` rather than in codegen emission logic
- **Snapshot churn**: All 7 snapshot tests require one-time update (task function bodies unchanged)

## Alternatives

- **Runtime class hierarchy** (`PipitPipeline` base class): Rejected — requires virtual dispatch overhead and changes task function signatures. Descriptor tables are zero-overhead data.
- **Code generation templates** (string templates with placeholders): Rejected — harder to maintain than structured emission, no type safety.
- **Separate `.cpp` compilation unit** for shell: Rejected — breaks the header-only library pattern established by `pipit.h`. Would require a build system change for all users.
- **Move globals into shell**: Rejected — task functions reference globals directly in hot loops. Indirection through pointers/accessors adds complexity without benefit.

## Exit criteria

- [ ] `pipit_shell.h` header with descriptor types and `shell_main()` implementation
- [ ] All 7 example PDL files compile through new codegen path
- [ ] C++ unit tests for `shell_main()` covering CLI parsing, probe validation, duration, stats
- [ ] Release mode: `--probe` and `--probe-output` accepted without error
- [ ] Snapshot tests updated with new generated output format
- [ ] Generated code compiles with `-fsyntax-only` for all examples (debug and release)
