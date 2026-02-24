# pcc Usage Guide

`pcc` is the Pipit Compiler Collection — compiles `.pdl` pipeline descriptions into C++ executables.

## Compiler Usage

```
pcc <source.pdl> -I <actors.h> [options]
```

**Flags:**

| Flag | Description |
|------|-------------|
| `-I <path>` | Actor header file or search directory (C++ with `ACTOR` macros) — repeatable |
| `--actor-path <dir>` | Search directory for actor headers — repeatable |
| `-o, --output <path>` | Output file path (default: `a.out` for exe, `-` for other formats) |
| `--actor-meta <path>` | Load actor metadata from manifest JSON (hermetic, no header scanning) |
| `--emit <format>` | Output stage: `exe` (default), `cpp`, `manifest`, `build-info`, `ast`, `schedule`, `graph-dot`, `timing-chart` |
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

# Use -I with a directory (discovers all headers recursively)
pcc examples/gain.pdl -I examples/ -o gain

# Use actor search path for automatic header discovery
pcc examples/gain.pdl --actor-path examples -o gain

# Release build with custom compiler flags
pcc examples/gain.pdl -I examples/actors.h --release --cflags "-O3 -march=native" -o gain
```

## Manifest-first Workflow

For hermetic, reproducible builds, use `--emit manifest` to generate actor metadata
separately from compilation:

```bash
# Step 1: Generate actor metadata from headers
pcc --emit manifest -I runtime/libpipit/include -I examples/ -o actors.meta.json

# Step 2: Compile using manifest (hermetic — no header scanning)
pcc source.pdl --actor-meta actors.meta.json --emit cpp -o source.cpp

# Step 3: Inspect build provenance
pcc --emit build-info source.pdl --actor-meta actors.meta.json
```

**`--emit manifest`** scans headers and outputs canonical `actors.meta.json`.
Does not require a `.pdl` source file. Cannot be combined with `--actor-meta` (usage error).

**`--emit build-info`** outputs provenance JSON (source hash, registry fingerprint,
compiler version). Does not require a valid parse — provenance is about "what went in",
not "does it compile".

**Provenance in generated C++**: When compiling, the first line of generated C++
includes a machine-parsable provenance comment:

```cpp
// pcc provenance: source_hash=<hex> registry_fingerprint=<hex> version=0.1.2
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
startup error: --probe requires a name
startup error: unknown probe 'probe_name'
startup error: --probe-output requires a path
startup error: failed to open probe output file '/path': <errno message>
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

## Probe Debugging

Probes are zero-cost observation points in PDL pipelines that emit data samples for debugging and validation. Probes are **completely stripped** from release builds (`--release`) with no runtime overhead.

### Adding Probes to PDL

Use the `?name` syntax in pipeline expressions to create a probe:

```pdl
clock 10MHz receiver {
    mode sync {
        adc(0) | fir(sync_coeff) | ?sync_out -> sync_result
    }

    mode data {
        adc(0) | fft(256) | c2r() | ?data_out -> payload
    }
}
```

Probes are passthrough nodes — they don't modify data flow, only observe it when explicitly enabled at runtime.

### Runtime Probe Control

**Enable specific probes:**

```bash
# Enable single probe (output to stderr)
./receiver --duration 1s --probe sync_out

# Enable multiple probes
./receiver --duration 1s --probe sync_out --probe data_out

# Duplicate probe names are idempotent (no error)
./receiver --duration 1s --probe sync_out --probe sync_out
```

**Redirect probe output to file:**

```bash
# Write probe data to file instead of stderr
./receiver --duration 1s --probe sync_out --probe-output /tmp/probes.txt

# Multiple probes to same file
./receiver --duration 1s --probe sync_out --probe data_out --probe-output /tmp/all_probes.txt
```

**Default behavior:**

- By default, **all probes are disabled** and produce no output
- Probes must be explicitly enabled with `--probe <name>` to emit data
- When `--probe-output` is not specified, probe data goes to **stderr**

### Probe Output Format

Each probe emits one line per token:

```
[probe:sync_out] 0.123456
[probe:sync_out] 0.234567
[probe:sync_out] 0.345678
```

Format: `[probe:<name>] <value>` where value depends on data type:

- `float`, `double`: printed as `%f`
- `int32`, `int16`, `int8`: printed as `%d`
- `cfloat`, `cdouble`: real part printed

### Probe Error Handling

**Unknown probe name:**

```bash
$ ./receiver --probe nonexistent
startup error: unknown probe 'nonexistent'
# Exit code: 2
```

**Missing probe output path:**

```bash
$ ./receiver --probe-output
startup error: --probe-output requires a path
# Exit code: 2
```

**File open failure:**

```bash
$ ./receiver --probe sync_out --probe-output /nonexistent/path/file.txt
startup error: failed to open probe output file '/nonexistent/path/file.txt': No such file or directory
# Exit code: 2
```

**Important:** Startup validation failures (unknown probe, file errors) **never launch worker threads**. The program exits immediately with code 2.

### Release Builds

Probes are **completely stripped** from release builds:

```bash
# Build with --release flag
pcc receiver.pdl -I actors.h --release -o receiver_release

# Probe infrastructure is not included (zero cost)
# --probe and --probe-output flags still parse but have no effect
```

In release builds:

- No probe storage variables generated (`_probe_*_enabled` flags omitted)
- No probe emission code (`#ifndef NDEBUG` guards)
- No runtime overhead — equivalent to code without probes
- CLI flags accepted but ignored (for deployment script compatibility)

### Probe Use Cases

**Validate signal processing:**

```bash
# Check FFT output magnitudes
./receiver --duration 0.1s --probe fft_mag --probe-output fft_samples.txt
```

**Debug mode switching:**

```bash
# Observe which mode is producing data
./receiver --duration 10s --probe sync_out --probe data_out
```

**Compare pipeline stages:**

```bash
# Multiple probes to track signal flow
clock 48kHz audio {
    adc(0) | ?raw | lpf(1000) | ?filtered | gain(:volume) | ?output | dac(0)
}

./audio --probe raw --probe filtered --probe output --probe-output stages.txt
```

### Example: Debugging receiver.pdl

```bash
# Compile receiver with probe support
pcc examples/receiver.pdl -I examples/actors.h -o receiver

# Run and observe sync mode output
./receiver --duration 0.1s --probe sync_out

# Capture data mode output to file
./receiver --duration 1s --probe data_out --probe-output data_samples.txt

# Enable both probes simultaneously
./receiver --duration 1s --probe sync_out --probe data_out

# Combine with statistics
./receiver --duration 1s --probe sync_out --probe-output probes.txt --stats
```
