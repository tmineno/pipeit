# Runtime Benchmarks

Benchmarks for `libpipit` runtime primitives: RingBuffer, Timer, TaskStats.

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
