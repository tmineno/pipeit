# ADR-010: RingBuffer False Sharing and Multi-Reader Contention Reduction

## Context

Benchmark analysis (v0.2.1) revealed severe multi-reader contention in `RingBuffer`. At 16 readers, cache miss rate rises from 59.24% to 68.67%, IPC degrades from 2.25 to 1.67, and latency regresses ~14x compared to 2 readers. The optimization backlog (TODO.md lines 275-285) identified two related targets:

1. Ring buffer contention optimization: rework multi-reader tail publication, add cache-line padding, evaluate batched publish strategy
2. Memory false-sharing tuning: eliminate false-sharing hotspots, audit memory layout alignment

Root cause analysis identified three issues in `pipit::RingBuffer` (pipit.h lines 56-113):

- **False sharing on reader tails**: `alignas(64) std::atomic<std::size_t> tails_[Readers]{}` aligns the *array start* to 64 bytes, but individual elements (8 bytes each) pack contiguously. For 8 readers, all tails occupy a single 64-byte cache line. Any reader's `store()` invalidates the cache line for all other readers.
- **O(Readers) writer overhead per write**: `write()` scans all reader tails with acquire loads on every call, even when the buffer has ample free space.
- **Per-element modulo copy**: `buf_[(h+i) % N] = src[i]` loop prevents compiler memcpy emission and defeats hardware prefetch.

## Decision

Implement three orthogonal fixes:

### 1. PaddedTail struct

Wrap each reader tail in a `struct alignas(64)` so each occupies its own cache line:

```cpp
struct alignas(64) PaddedTail {
    std::atomic<std::size_t> value{0};
};
PaddedTail tails_[Readers];
```

Memory cost: +56 bytes per reader (acceptable — data buffer is 4K-64K elements).

### 2. Cached min_tail

Add writer-private `std::size_t cached_min_tail_{0}`. Fast path checks against cached value; slow path rescans all tails only when buffer appears full:

```cpp
std::size_t used = h - cached_min_tail_;
if (used > Capacity || Capacity - used < count) {
    // Slow path: rescan all tails
    ...
}
```

Correctness: `cached_min_tail_` is always ≤ real min_tail (readers only advance). Stale cache triggers unnecessary rescan but never overflows. O(1) amortized per write.

### 3. Two-phase memcpy

Replace per-element modulo loop with at-most-2 `std::memcpy` calls:

```cpp
std::size_t start = h % N;
std::size_t first = std::min(count, N - start);
std::memcpy(&buf_[start], src, first * sizeof(T));
if (first < count)
    std::memcpy(&buf_[0], src + first, (count - first) * sizeof(T));
```

Added `static_assert(std::is_trivially_copyable_v<T>)` as safety guard. All current Pipit types (float, cfloat, int32, double) satisfy this.

## Consequences

- Memory per reader tail increases from 8 bytes to 64 bytes (padding)
- Writer fast-path becomes O(1) amortized instead of O(Readers)
- Data copy eliminates per-element modulo, enables hardware prefetch
- API surface unchanged (`write`, `read`, `available` signatures preserved)
- Generated C++ requires no changes (codegen uses public API only)

## Alternatives

- **DPDK-style per-reader ring**: Each reader gets its own ring buffer copy. Higher memory, different API, overkill for ≤16 readers.
- **Bitfield-compressed tails**: Pack tail progress into fewer cache lines. Complex implementation, marginal gain over padded layout.
- **Reader-publisher aggregation thread**: Dedicated thread aggregates tail updates. Adds latency and complexity.
- **Seqlock on min_tail**: Writer publishes min_tail under seqlock, readers check locally. Adds complexity; cached approach is simpler with same amortized cost.

## Exit criteria

- [ ] 16-reader latency regression reduced by at least 2x (from ~14x to ≤7x vs 2-reader)
- [ ] 8-reader false sharing latency reduced by at least 30%
- [ ] All existing tests pass (378+)
- [ ] `BM_Memory_Footprint` counters reflect new padded sizes
- [ ] `BM_RingBuffer_Contention/32readers` runs without crash
