#!/bin/bash
# perf_profile.sh — CPU hotspot, branch, cache, TLB analysis
#
# Runs comprehensive profiling across all benchmark binaries.
# Outputs structured text summaries.

set -e
source "$(dirname "$0")/perf_common.sh"

mkdir -p "$BUILD_DIR" "$OUTPUT_DIR"
trap 'rm -rf "$BUILD_DIR"' EXIT

print_header "Profiling & Analysis"

# Build all benchmark binaries
RINGBUF_EXE=$(build_bench "$BENCH_DIR/ringbuf_bench.cpp" "ringbuf_bench")
ACTOR_EXE=$(build_bench "$BENCH_DIR/actor_bench.cpp" "actor_bench")
THREAD_EXE=$(build_bench "$BENCH_DIR/thread_bench.cpp" "thread_bench")

declare -A TARGETS=(
    ["ringbuf"]="$RINGBUF_EXE"
    ["actor"]="$ACTOR_EXE"
    ["thread"]="$THREAD_EXE"
)

for name in "${!TARGETS[@]}"; do
    exe="${TARGETS[$name]}"
    echo ""
    echo -e "${GREEN}=== Profile: $name ===${NC}"

    # ── CPU hotspots (perf record + perf report) ─────────────────────────

    echo "--- CPU Hotspots ---"
    PERF_DATA="$OUTPUT_DIR/perf_hotspot_${name}.data"
    if perf record -g -o "$PERF_DATA" -- "$exe" --benchmark_repetitions=1 2>/dev/null; then
        perf report -i "$PERF_DATA" --stdio --no-children --sort=dso,symbol \
            --percent-limit 1.0 2>/dev/null > "$OUTPUT_DIR/perf_hotspot_${name}.txt" || true
        echo "  Saved: perf_hotspot_${name}.txt"
        # Clean up perf.data (can be large)
        rm -f "$PERF_DATA"
    else
        echo "  WARNING: perf record failed (may need elevated privileges)"
    fi

    # ── Branch mispredictions ────────────────────────────────────────────

    echo "--- Branch Mispredictions ---"
    if [ -n "$AVAIL_BRANCH_EVENTS" ]; then
        perf_stat_text "$AVAIL_BRANCH_EVENTS" "$exe" "--benchmark_repetitions=1" 3
    else
        echo "  WARNING: No branch events available."
    fi

    # ── Cache miss rates (L1/LLC) ────────────────────────────────────────

    echo "--- Cache Miss Rates ---"
    if [ -n "$AVAIL_CACHE_EVENTS" ]; then
        perf_stat_text "$AVAIL_CACHE_EVENTS" "$exe" "--benchmark_repetitions=1" 3
    else
        echo "  WARNING: No cache events available."
    fi

    # ── TLB misses ───────────────────────────────────────────────────────

    echo "--- TLB Misses ---"
    if [ -n "$AVAIL_TLB_EVENTS" ]; then
        perf_stat_text "$AVAIL_TLB_EVENTS" "$exe" "--benchmark_repetitions=1" 3
    else
        echo "  WARNING: No TLB events available."
    fi

    echo ""
done

echo "=== Profiling & analysis complete ==="
echo "Results in: $OUTPUT_DIR/"
