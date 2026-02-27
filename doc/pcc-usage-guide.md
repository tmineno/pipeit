# pcc Usage Guide

`pcc` compiles Pipit `.pdl` programs into generated C++ or native executables.

## CLI Synopsis

```bash
pcc [source.pdl] [options]
```

- `source.pdl` is required for all stages except `--emit manifest`.
- `--emit manifest` runs without a source file.

## Compiler Options

| Flag | Description |
|------|-------------|
| `-o, --output <path>` | Output path. Defaults by stage (see table below). |
| `-I, --include <path>` | Actor header file or directory (repeatable). |
| `--actor-path <dir>` | Recursive actor header search directory (repeatable; directory required). |
| `--actor-meta <file>` | Actor metadata manifest (`actors.meta.json`). |
| `--emit <stage>` | `exe` (default), `cpp`, `ast`, `graph`, `graph-dot`, `schedule`, `timing-chart`, `manifest`, `build-info`. |
| `--release` | Release codegen profile (probe stripping + optimized C++ defaults). |
| `--cc <compiler>` | C++ compiler command for `--emit exe` (default: `clang++`). |
| `--cflags "<flags>"` | Extra C++ flags. Overrides default optimization flags (`-O0 -g` debug, `-O2` release). |
| `--diagnostic-format <human\|json>` | Diagnostic output format (default: `human`). |
| `--verbose` | Print phase/timing trace information. |
| `--help`, `--version` | Standard CLI help/version output. |

## Emit Stages and Output Behavior

| Stage | Source required | Output destination | Notes |
|------|------------------|--------------------|------|
| `exe` | yes | `a.out` by default, or `-o` | Invokes system C++ compiler. |
| `cpp` | yes | stdout by default, or `-o` | Generated C++ only. |
| `manifest` | no | stdout by default, or `-o` | Cannot be combined with `--actor-meta`. |
| `build-info` | yes | stdout by default, or `-o` | Uses source text + registry; does not require successful parse. |
| `ast` | yes | stdout | Parsed AST debug dump. |
| `graph` | yes | stdout | Graph/analyze dump. |
| `graph-dot` | yes | stdout | Graphviz DOT output. |
| `schedule` | yes | stdout | Schedule dump. |
| `timing-chart` | yes | stdout | Mermaid Gantt chart. |

## Actor Metadata Loading Rules

- With `--actor-meta`, metadata is loaded from manifest only.
- Without `--actor-meta`, metadata is built by scanning headers.
- In header-scan mode, `--actor-path` is the base and `-I` overlays with higher precedence on name conflicts.
- Even with `--actor-meta`, `-I` / `--actor-path` are still used to collect header includes for generated C++ compilation inputs.

## Common Workflows

### 1) Compile to executable (single step)

```bash
pcc examples/gain.pdl \
  -I runtime/libpipit/include/std_actors.h \
  --cflags "-std=c++20 -O2" \
  -o gain
```

Note: generated code and runtime headers use `std::span`; use a C++20-capable toolchain.

### 2) Emit generated C++ (stdout)

```bash
pcc examples/gain.pdl -I runtime/libpipit/include/std_actors.h --emit cpp
```

### 3) Emit generated C++ (file)

```bash
pcc examples/gain.pdl -I runtime/libpipit/include/std_actors.h --emit cpp -o gain.cpp
```

### 4) Generate manifest for hermetic builds

```bash
pcc --emit manifest \
  -I runtime/libpipit/include \
  -I examples \
  -o actors.meta.json
```

### 5) Compile using manifest metadata

```bash
pcc examples/gain.pdl \
  --actor-meta actors.meta.json \
  -I runtime/libpipit/include \
  -I examples \
  --emit cpp -o gain.cpp
```

### 6) Emit provenance build info

```bash
pcc examples/gain.pdl \
  --actor-meta actors.meta.json \
  --emit build-info
```

### 7) Machine-readable diagnostics

```bash
pcc bad.pdl -I examples --diagnostic-format json
```

## Compiler Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Compilation failure (parse/resolve/type/analyze/schedule/codegen error) |
| `2` | Usage failure (invalid args, missing required input, incompatible flags) |
| `3` | System failure (I/O/tool invocation failure) |

## Generated Binary Runtime Flags

Generated executables accept:

```bash
./program [--duration <time>] [--param name=value] [--stats] [--probe <name>] [--probe-output <path>] [--threads <n>]
```

| Flag | Description |
|------|-------------|
| `--duration <time>` | Duration (`10s`, `1m`, `inf`, or bare seconds like `0.5`). Default: run until SIGINT. |
| `--param name=value` | Runtime parameter override. |
| `--stats` | Print per-task and buffer statistics. |
| `--probe <name>` | Enable a named probe. Repeatable. |
| `--probe-output <path>` | Probe output file path (default sink: stderr). |
| `--threads <n>` | Advisory thread hint. |

Runtime startup failures return exit code `2` (invalid flags, bad values, unknown probe/param, file-open errors).

When `--threads` is provided with fewer threads than tasks, runtime prints an advisory warning.

## Probe Behavior

- Probes are disabled by default.
- Enable probes explicitly with `--probe <name>`.
- In release codegen (`pcc --release`), probe descriptors are stripped; probe flags are accepted but have no effect.

Example:

```bash
./receiver --duration 1s --probe sync_out --probe-output probes.txt --stats
```

## Overrun Policy

Set in `.pdl` via `set overrun = <policy>`:

- `drop` (default)
- `slip`
- `backlog`
