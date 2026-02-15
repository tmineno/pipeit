#!/bin/bash
# perf_affinity.sh — CPU affinity impact analysis with perf
#
# Wraps affinity_bench with perf stat to correlate pinning strategy
# with cache behavior, context switches, and CPU migrations.

set -e
source "$(dirname "$0")/perf_common.sh"

mkdir -p "$BUILD_DIR" "$OUTPUT_DIR"
trap 'rm -rf "$BUILD_DIR"' EXIT

print_header "CPU Affinity Impact"

EXE=$(build_bench "$BENCH_DIR/affinity_bench.cpp" "affinity_bench")

# Build event set from probed capabilities
EVENTS=""
[ -n "$AVAIL_HW_EVENTS" ] && EVENTS="$AVAIL_HW_EVENTS"
probe_event "cache-misses" && EVENTS="${EVENTS:+$EVENTS,}cache-misses"
probe_event "context-switches" && EVENTS="${EVENTS:+$EVENTS,}context-switches"
probe_event "cpu-migrations" && EVENTS="${EVENTS:+$EVENTS,}cpu-migrations"

if [ -z "$EVENTS" ]; then
    echo "WARNING: No events available. Using cpu-clock fallback."
    EVENTS="cpu-clock"
fi

for filter in "Unpinned" "SameCore" "AdjacentCore" "DistantCore"; do
    echo "--- $filter ---"
    perf_stat_text "$EVENTS" \
        "$EXE" "--benchmark_filter=$filter --benchmark_repetitions=1" 3
    echo ""
done

echo "--- Task Scaling with Affinity ---"
perf_stat_text "$EVENTS" \
    "$EXE" "--benchmark_filter=TaskScaling --benchmark_repetitions=1" 3

echo ""
echo "=== CPU affinity analysis complete ==="
echo "Results in: $OUTPUT_DIR/"
