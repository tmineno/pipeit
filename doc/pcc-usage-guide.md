# pcc Usage Guide

`pcc` is the Pipit Compiler Collection — compiles `.pdl` pipeline descriptions into C++ executables.

## Compiler Usage

```
pcc <source.pdl> -I <actors.h> [--emit <format>] [--release]
```

**Flags:**

| Flag | Description |
|------|-------------|
| `-I <path>` | Actor header file (C++ with `ACTOR` macros) |
| `--emit cpp` | Emit generated C++ source to stdout |
| `--emit schedule` | Emit computed PASS schedule |
| `--emit graph-dot` | Emit Graphviz DOT dataflow graph |
| `--emit timing-chart` | Emit Mermaid Gantt timing chart |
| `--release` | Strip probe instrumentation for zero-cost production builds |

**Example:**

```bash
pcc examples/gain.pdl -I examples/actors.h --emit cpp > gain.cpp
c++ -std=c++20 -O2 gain.cpp -I runtime/libpipit/include -I examples -lpthread -o gain
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

**Exit codes:**

| Code | Meaning |
|------|---------|
| 0 | Normal exit |
| 1 | Runtime error (actor returned `ACTOR_ERROR`) |
| 2 | Startup error (unknown flag, bad param, invalid probe name) |

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
