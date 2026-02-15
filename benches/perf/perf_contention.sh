#!/bin/bash
# perf_contention.sh — Atomic contention and memory ordering analysis
#
# Lock contention in lock-free RingBuffer:
# - Atomic contention under multi-reader load
# - Memory ordering overhead (acquire/release cost)
# - Lock-free algorithm efficiency (cycles per operation)

set -e
source "$(dirname "$0")/perf_common.sh"

mkdir -p "$BUILD_DIR" "$OUTPUT_DIR"
trap 'rm -rf "$BUILD_DIR"' EXIT

print_header "Lock Contention & Atomic Analysis"

RINGBUF_EXE=$(build_bench "$BENCH_DIR/ringbuf_bench.cpp" "ringbuf_bench")

# Build event set from probed capabilities
EVENTS=""
probe_event "cpu-cycles" && EVENTS="cpu-cycles"
probe_event "instructions" && EVENTS="${EVENTS:+$EVENTS,}instructions"
probe_event "cache-references" && EVENTS="${EVENTS:+$EVENTS,}cache-references"
probe_event "cache-misses" && EVENTS="${EVENTS:+$EVENTS,}cache-misses"
probe_event "stalled-cycles-frontend" && EVENTS="${EVENTS:+$EVENTS,}stalled-cycles-frontend"
probe_event "context-switches" && EVENTS="${EVENTS:+$EVENTS,}context-switches"

if [ -z "$EVENTS" ]; then
    echo "WARNING: No events available. Cannot perform contention analysis."
    exit 0
fi

# ── Atomic contention scaling ────────────────────────────────────────────
# As reader count increases, atomic operations on head_ face more
# contention. Track cache-misses and stalled-cycles as proxies.

echo "=== Atomic Contention Scaling ==="
echo "(Watch cache-misses and stalled-cycles as reader count grows)"
echo ""

for readers in 2 4 8 16; do
    echo "--- $readers readers ---"
    perf_stat_text "$EVENTS" "$RINGBUF_EXE" \
        "--benchmark_filter=Contention/${readers}readers --benchmark_repetitions=1" 5
    echo ""
done

# ── Single-threaded baseline (no contention) ─────────────────────────────

echo "=== Baseline: No Contention (single-threaded) ==="
perf_stat_text "$EVENTS" "$RINGBUF_EXE" \
    "--benchmark_filter=SizeScaling/4K --benchmark_repetitions=1" 5

# ── Memory ordering overhead ─────────────────────────────────────────────
# Compare single-reader (minimal barriers) vs multi-reader.

echo ""
echo "=== Memory Ordering: 1-reader vs Multi-reader ==="
echo "(Difference reveals memory barrier overhead)"
echo ""

MO_EVENTS=""
probe_event "cpu-cycles" && MO_EVENTS="cpu-cycles"
probe_event "instructions" && MO_EVENTS="${MO_EVENTS:+$MO_EVENTS,}instructions"
probe_event "stalled-cycles-frontend" && MO_EVENTS="${MO_EVENTS:+$MO_EVENTS,}stalled-cycles-frontend"

if [ -n "$MO_EVENTS" ]; then
    echo "--- 1 reader (minimal ordering overhead) ---"
    perf_stat_text "$MO_EVENTS" "$RINGBUF_EXE" \
        "--benchmark_filter=Throughput --benchmark_repetitions=1" 5

    echo ""
    echo "--- 2 readers (acquire/release on each tail) ---"
    perf_stat_text "$MO_EVENTS" "$RINGBUF_EXE" \
        "--benchmark_filter=Contention/2readers --benchmark_repetitions=1" 5
fi

echo ""
echo "=== Lock contention analysis complete ==="
echo "Results in: $OUTPUT_DIR/"
