#!/bin/bash
# perf_ringbuf.sh — Ring buffer cache hit/miss analysis
#
# Measures L1/L2/L3 cache behavior during ring buffer operations.
# Wraps the existing ringbuf_bench binary with perf stat.

set -e
source "$(dirname "$0")/perf_common.sh"

mkdir -p "$BUILD_DIR" "$OUTPUT_DIR"
trap 'rm -rf "$BUILD_DIR"' EXIT

print_header "Ring Buffer Cache Effects"

# Build the existing ring buffer benchmark
EXE=$(build_bench "$BENCH_DIR/ringbuf_bench.cpp" "ringbuf_bench")

# ── L1 data cache across buffer sizes ────────────────────────────────────

if [ -n "$AVAIL_CACHE_EVENTS" ]; then
    echo "--- L1 Data Cache (by buffer size) ---"
    for filter in "SizeScaling/64" "SizeScaling/256" "SizeScaling/1K" "SizeScaling/4K" "SizeScaling/16K" "SizeScaling/64K"; do
        # Annotate which cache level this buffer fits in
        case "$filter" in
            */64)   sz=$((64 * 4)); level=$(cache_level_for_size $sz) ;;
            */256)  sz=$((256 * 4)); level=$(cache_level_for_size $sz) ;;
            */1K)   sz=$((1024 * 4)); level=$(cache_level_for_size $sz) ;;
            */4K)   sz=$((4096 * 4)); level=$(cache_level_for_size $sz) ;;
            */16K)  sz=$((16384 * 4)); level=$(cache_level_for_size $sz) ;;
            */64K)  sz=$((65536 * 4)); level=$(cache_level_for_size $sz) ;;
        esac
        echo ""
        echo "  $filter (buffer fits in: $level)"
        perf_stat_text "$AVAIL_CACHE_EVENTS" \
            "$EXE" "--benchmark_filter=$filter --benchmark_repetitions=1" 3
    done
else
    echo "  WARNING: No cache events available. Skipping L1 analysis."
fi

# ── LLC (Last Level Cache) across buffer sizes ───────────────────────────

echo ""
echo "--- Last Level Cache (L3) ---"
LLC_EVENTS=""
probe_event "cache-references" && LLC_EVENTS="cache-references"
probe_event "cache-misses" && LLC_EVENTS="${LLC_EVENTS:+$LLC_EVENTS,}cache-misses"

if [ -n "$LLC_EVENTS" ]; then
    for filter in "SizeScaling/64" "SizeScaling/4K" "SizeScaling/64K"; do
        echo "  $filter"
        perf_stat_text "$LLC_EVENTS" \
            "$EXE" "--benchmark_filter=$filter --benchmark_repetitions=1" 3
    done
else
    echo "  WARNING: No LLC events available."
fi

# ── TLB analysis ────────────────────────────────────────────────────────

echo ""
echo "--- TLB ---"
if [ -n "$AVAIL_TLB_EVENTS" ]; then
    perf_stat_text "$AVAIL_TLB_EVENTS" \
        "$EXE" "--benchmark_filter=SizeScaling --benchmark_repetitions=1" 3
else
    echo "  WARNING: No TLB events available."
fi

# ── Multi-reader contention + cache ──────────────────────────────────────

echo ""
echo "--- Multi-Reader Contention + Cache ---"
COMBINED=""
[ -n "$AVAIL_HW_EVENTS" ] && COMBINED="$AVAIL_HW_EVENTS"
[ -n "$AVAIL_CACHE_EVENTS" ] && COMBINED="${COMBINED:+$COMBINED,}$AVAIL_CACHE_EVENTS"

if [ -n "$COMBINED" ]; then
    for readers in 2 4 8 16; do
        echo ""
        echo "  $readers readers"
        perf_stat_text "$COMBINED" \
            "$EXE" "--benchmark_filter=Contention/${readers}readers --benchmark_repetitions=1" 3
    done
else
    echo "  WARNING: No hardware/cache events available."
fi

echo ""
echo "=== Ring buffer cache analysis complete ==="
echo "Results in: $OUTPUT_DIR/"
