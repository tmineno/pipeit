# SHM Example — Shared-Memory Data Exchange

Two-process example demonstrating PSHM (Pipit Shared Memory) bind transport.

## Overview

- **writer.pdl** — Generates a 1 kHz sine wave at 48 kHz and publishes
  256-sample blocks to a shared-memory ring named `pipit_demo`.
- **reader.pdl** — Attaches to the same shared-memory ring and prints
  received samples to stdout.

Both programs use the same ring geometry (`slots=64, slot_bytes=1024`),
which is validated at attach time.

## Quick Start

```bash
# From the repository root:
./examples/shm/run.sh
```

The script builds both programs, runs the writer in the background,
starts the reader, and verifies that non-zero samples were received.

## Manual Run

```bash
# Build
cargo build -p pcc --release
./examples/build.sh

# Terminal 1 — writer
./examples/build/shm_writer --duration 5s

# Terminal 2 — reader (start after writer)
./examples/build/shm_reader --duration 2s
```

## Runtime Rebind

Override the shm object name at runtime with `--bind`:

```bash
./examples/build/shm_writer --bind sig=my_ring --duration 5s &
sleep 1
./examples/build/shm_reader --bind sig=my_ring --duration 2s
```
