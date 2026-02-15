# Runtime Benchmarks

This directory contains two types of benchmarks:

1. **Primitive Benchmarks** (`runtime_bench.cpp`) - Low-level performance of `libpipit` primitives
2. **End-to-End Benchmarks** (`pdl_bench.sh`) - Full pipeline performance from compiled PDL programs

## Requirements

- Google Benchmark library: https://github.com/google/benchmark

Install on Ubuntu/Debian:

```bash
sudo apt install libbenchmark-dev
```

## Build and Run

```bash
# Compile runtime benchmarks
c++ -std=c++20 -O3 -march=native \
    -I ../runtime/libpipit/include \
    runtime_bench.cpp \
    -lbenchmark -lpthread \
    -o runtime_bench

# Run benchmarks
./runtime_bench

# Run with specific filters
./runtime_bench --benchmark_filter=RingBuffer

# Output as JSON
./runtime_bench --benchmark_format=json --benchmark_out=results.json
```

## Benchmark Suite

### RingBuffer

- `BM_RingBuffer_Write` - Write performance (64 floats per iteration)
- `BM_RingBuffer_Read` - Read performance (64 floats per iteration)
- `BM_RingBuffer_RoundTrip` - Write/read latency (32 floats)
- `BM_RingBuffer_MultiReader` - Multi-reader performance (2 readers, 16 floats)

### Timer

- `BM_Timer_Tick_1kHz` - 1 kHz timer tick overhead
- `BM_Timer_Tick_10kHz` - 10 kHz timer tick overhead

### TaskStats

- `BM_TaskStats_Record` - Statistics recording overhead

### Atomic Parameters

- `BM_AtomicParam_Load` - Runtime parameter read overhead
- `BM_AtomicParam_Store` - Runtime parameter write overhead

## Expected Performance

On modern hardware (x86_64, 3+ GHz):

- RingBuffer write: ~10-20 ns per token
- RingBuffer read: ~10-20 ns per token
- Timer tick (1 kHz): ~1-5 µs overhead
- TaskStats record: ~5-10 ns
- Atomic param load/store: ~1-2 ns

---

## PDL End-to-End Benchmarks

### Quick Start

```bash
# Run all PDL benchmarks (compiles and executes test programs)
./pdl_bench.sh
```

### Test PDL Programs

The `pdl/` directory contains representative pipeline programs:

- **simple.pdl** - Single task, minimal overhead baseline (100 kHz)
- **multitask.pdl** - Multi-task with shared buffer communication (10 kHz producer, 5 kHz consumer)
- **modal.pdl** - CSDF mode switching overhead (50 kHz)
- **complex.pdl** - Realistic processing with taps, shared buffers, decimation (100 kHz + 10 kHz)

### What Gets Measured

For each PDL program:

1. Compilation time (pcc → C++)
2. C++ compilation time (C++ → executable)
3. Runtime performance:
   - Tick counts and miss rates
   - Average/max task latency
   - Shared buffer utilization

### Manual Profiling

```bash
# Compile and run individual PDL programs manually
cd pdl
../../target/release/pcc simple.pdl -I ../../examples/actors.h -o /tmp/simple
/tmp/simple --duration 10s --stats

# With custom parameters
../../target/release/pcc modal.pdl -I ../../examples/actors.h -o /tmp/modal
/tmp/modal --duration 2s --param mode_sel=1 --stats
```

### Build Artifacts

All generated C++ files and compiled binaries are placed in `/tmp/pipit_bench_<pid>/` and automatically cleaned up after benchmarking. This keeps the source directory clean.

### Expected End-to-End Performance

On modern hardware (x86_64, 3+ GHz):

- Simple pipeline (100 kHz): ~70-80 µs avg latency per tick
- Multi-task (10 kHz/5 kHz): ~75-85 µs avg latency per task
- Modal switching (50 kHz): ~70-80 µs avg latency with mode transitions
- Complex pipeline (100 kHz): ~70-90 µs avg latency with decimation

These end-to-end latencies include:

- Actor execution time
- Ring buffer operations
- Timer wait overhead
- Statistics collection (if enabled)
- Mode switching (for modal pipelines)
