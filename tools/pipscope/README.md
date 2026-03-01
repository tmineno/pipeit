# pipscope

Real-time oscilloscope GUI for Pipit signal processing pipelines.

Receives waveform data via **PPKT** (UDP packets) and/or **PSHM** (POSIX shared memory) and displays multi-channel waveforms using ImGui + ImPlot.

## Build

Requires: CMake 3.20+, C++20, OpenGL 3.3, a display server (X11/Wayland/WSLg).

Dependencies (fetched automatically via `FetchContent`): GLFW 3.4, Dear ImGui 1.91.8, ImPlot 0.16.

```bash
cd tools/pipscope
cmake -B build
cmake --build build
```

## Usage

```
pipscope [--port <port>] [--address <addr>] [--shm <name>] [options]
```

### Options

| Flag | Description |
|------|-------------|
| `-p, --port <port>` | Listen on `0.0.0.0:<port>` for PPKT/UDP packets |
| `-a, --address <addr>` | Listen on `<addr>` (e.g. `localhost:9100`) |
| `--shm <name>` | Attach to a PSHM ring by name (repeatable) |
| `--vsync` | Enable vsync (default: off) |
| `--snapshot-hz <N>` | Limit snapshot rate to N Hz (0 = unlimited) |
| `-h, --help` | Show help |

If no address is given, the GUI starts with a text input for on-the-fly connection.

### Examples

**PPKT/UDP source** — receive from a Pipit pipeline using `socket_write`:

```bash
# Terminal 1: run a pipeline that streams via UDP
./socket_stream --duration 10s

# Terminal 2: view waveforms
./build/pipscope --port 9100
```

**Shared memory source** — monitor PSHM rings directly:

```bash
# Terminal 1: run a pipeline that writes to SHM
./shm_scope --duration 30s

# Terminal 2: view waveforms
./build/pipscope --shm scope_ch0 --shm scope_ch1
```

**Mixed sources** — PPKT and SHM simultaneously:

```bash
./build/pipscope --port 9100 --shm scope_ch0
```

## GUI Controls

- **Pause / Resume** — freeze the display
- **Auto-Y** — auto-fit vertical axis to data range
- **Samples** — number of samples in the display window (64–65536, logarithmic)
- **Connect** — bind to a PPKT/UDP address at runtime
- **Trigger** — edge trigger with configurable level, edge direction (rising/falling), mode (Auto/Normal), and source channel

The status bar shows refresh rate, packet rate, throughput, and snapshot latency.

Channels with >5% drop rate or inter-frame gap rate display an **UNCAL** overlay.

## Architecture

All headers are inline (no `.cpp` files besides `main.cpp`).

| File | Role |
|------|------|
| `main.cpp` | GLFW/ImGui/ImPlot setup, render loop, snapshot pipeline |
| `types.h` | Shared data types: `SampleBuffer`, `ChannelSnapshot`, `FrameStats`, dtype conversions |
| `ppkt_receiver.h` | PPKT/UDP receiver — background thread, frame reassembly, channel demux |
| `shm_receiver.h` | SHM receiver — Superblock auto-discovery via `probe_shm()`, background poll using `ShmReader` |
| `trigger.h` | Edge trigger logic (pure functions, no dependencies) |
| `decimate.h` | Min/max envelope decimation for efficient rendering of large waveforms |
| `cli.h` | CLI argument parsing |

### Data flow

```
PPKT/UDP ──► PpktReceiver ──► SampleBuffer ──┐
                                              ├──► snapshot ──► trigger ──► extract_window / take_tail ──► ImPlot
PSHM ring ──► ShmReceiver ──► SampleBuffer ──┘
```

Both receiver types produce `ChannelSnapshot` structs that feed into the same trigger and windowing pipeline.

### SHM channel IDs

SHM channels are assigned deterministic IDs via FNV-1a hash of the ring name, mapped to range `0x8001–0xFFFF`. Collisions are detected at startup and resolved with salt rehashing.

## Tests

```bash
cmake --build build
ctest --test-dir build
```

| Test suite | What it covers |
|------------|----------------|
| `PpktReceiver` | Frame reassembly, dtype conversion, multi-channel, overflow, metrics |
| `ShmReceiver` | Superblock probe, attach/detach lifecycle, float/int16 conversion, overflow fast-forward, epoch fence recovery, channel ID hashing, label propagation |
