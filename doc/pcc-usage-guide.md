# pcc Usage Guide

`pcc` is the Pipit Compiler Collection — compiles `.pdl` pipeline descriptions into C++ executables.

## Compiler Usage

```
pcc <source.pdl> -I <actors.h> [options]
```

**Flags:**

| Flag | Description |
|------|-------------|
| `-I <path>` | Actor header file (C++ with `ACTOR` macros) — repeatable |
| `--actor-path <dir>` | Search directory for actor headers — repeatable |
| `-o, --output <path>` | Output file path (default: `a.out` for exe, `-` for other formats) |
| `--emit <format>` | Output stage: `exe` (default), `cpp`, `schedule`, `graph-dot`, `timing-chart` |
| `--release` | Strip probe instrumentation for zero-cost production builds |
| `--cc <compiler>` | C++ compiler command (default: `c++`) |
| `--cflags <flags>` | Additional C++ compiler flags (overrides default `-O2`) |
| `--verbose` | Print compiler phases and timing |

**Compiler Exit Codes:**

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Compilation error (parse error, type error, analysis failure) |
| 2 | Usage error (invalid arguments) |
| 3 | System error (I/O failure, missing files) |

**Examples:**

```bash
# Generate C++ source to stdout
pcc examples/gain.pdl -I examples/actors.h --emit cpp -o -

# Generate C++ to file
pcc examples/gain.pdl -I examples/actors.h --emit cpp -o gain.cpp

# Compile directly to executable (default)
pcc examples/gain.pdl -I examples/actors.h -o gain

# Use actor search path for automatic header discovery
pcc examples/gain.pdl --actor-path examples -o gain

# Release build with custom compiler flags
pcc examples/gain.pdl -I examples/actors.h --release --cflags "-O3 -march=native" -o gain
```

## Generated Binary Usage

The generated executable accepts runtime flags:

```
./gain [--duration <time>] [--param name=value] [--stats] [--probe <name>] [--probe-output <path>] [--threads <n>]
```

**Flags:**

| Flag | Description |
|------|-------------|
| `--duration <time>` | Run duration: `10s`, `1m`, `inf`, or bare seconds (`0.5`). Default: run until SIGINT. |
| `--param name=value` | Override a runtime parameter at startup |
| `--stats` | Print per-task statistics on exit (ticks, missed, latency) |
| `--probe <name>` | Enable a named probe for data observation |
| `--probe-output <path>` | Redirect probe output to file (default: stderr) |
| `--threads <n>` | Thread count hint (informational in v0.1) |

**Exit Codes:**

| Code | Meaning |
|------|---------|
| 0 | Normal exit (duration reached or SIGINT) |
| 1 | Runtime error (actor returned `ACTOR_ERROR`) |
| 2 | Startup error (unknown flag, invalid param, missing argument) |

**Error Messages:**

Startup errors include helpful context:

```
startup error: --param requires name=value
startup error: unknown param 'xyz'
startup error: --duration requires a value
startup error: invalid --duration '10x' (use <sec>, <sec>s, <min>m, or inf)
startup error: --threads requires a positive integer
startup error: unknown option '--bad-flag'
```

**Examples:**

```bash
# Run for 10 seconds with custom gain
./gain --duration 10s --param gain=3.5

# Run with statistics output
./gain --duration 1m --stats

# Enable probe observation
./gain --duration 5s --probe filtered --probe-output /tmp/probe.log
```

## Overrun Policies

Set in the `.pdl` source with `set overrun = <policy>`:

| Policy | Behavior |
|--------|----------|
| `drop` (default) | Skip missed ticks, continue at next scheduled tick |
| `slip` | Re-anchor timer to current time on overrun |
| `backlog` | Catch up by running extra iterations for missed ticks |

## Statistics Output

When `--stats` is enabled, statistics are printed to stderr on exit:

```
[stats] task 'audio': ticks=48000, missed=12 (drop), max_latency=1234ns, avg_latency=456ns
[stats] shared buffer 'signal': 256 tokens (1024B)
```
