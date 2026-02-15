#!/bin/bash
# perf_memory.sh — Memory subsystem analysis with perf
#
# Wraps memory_bench with perf stat for page faults, cache line
# utilization, false sharing, and bandwidth counters.

set -e
source "$(dirname "$0")/perf_common.sh"

mkdir -p "$BUILD_DIR" "$OUTPUT_DIR"
trap 'rm -rf "$BUILD_DIR"' EXIT

print_header "Memory Subsystem Analysis"

EXE=$(build_bench "$BENCH_DIR/memory_bench.cpp" "memory_bench")

# ── Footprint ────────────────────────────────────────────────────────────

echo "--- Memory Footprint ---"
echo "(Reported via benchmark counters — run memory_bench directly for details)"
echo ""

# ── Page fault analysis ──────────────────────────────────────────────────

echo "--- Page Fault Impact ---"
PF_EVENTS=""
probe_event "page-faults" && PF_EVENTS="page-faults"
probe_event "minor-faults" && PF_EVENTS="${PF_EVENTS:+$PF_EVENTS,}minor-faults"
probe_event "major-faults" && PF_EVENTS="${PF_EVENTS:+$PF_EVENTS,}major-faults"

if [ -n "$PF_EVENTS" ]; then
    perf_stat_text "$PF_EVENTS" \
        "$EXE" "--benchmark_filter=PageFault --benchmark_repetitions=1" 3
else
    echo "  WARNING: No page fault events available."
fi
echo ""

# ── Cache line utilization ───────────────────────────────────────────────

echo "--- Cache Line Utilization ---"
CL_EVENTS=""
probe_event "L1-dcache-loads" && CL_EVENTS="L1-dcache-loads"
probe_event "L1-dcache-load-misses" && CL_EVENTS="${CL_EVENTS:+$CL_EVENTS,}L1-dcache-load-misses"
probe_event "L1-dcache-prefetches" && CL_EVENTS="${CL_EVENTS:+$CL_EVENTS,}L1-dcache-prefetches"

if [ -n "$CL_EVENTS" ]; then
    perf_stat_text "$CL_EVENTS" \
        "$EXE" "--benchmark_filter=CacheLineUtil --benchmark_repetitions=1" 3
else
    echo "  WARNING: No L1 cache events available."
fi
echo ""

# ── False sharing detection ──────────────────────────────────────────────

echo "--- False Sharing Detection ---"
echo "(Compare cache miss rate as reader count grows)"
FS_EVENTS=""
probe_event "cpu-cycles" && FS_EVENTS="cpu-cycles"
probe_event "L1-dcache-load-misses" && FS_EVENTS="${FS_EVENTS:+$FS_EVENTS,}L1-dcache-load-misses"
probe_event "cache-references" && FS_EVENTS="${FS_EVENTS:+$FS_EVENTS,}cache-references"
probe_event "cache-misses" && FS_EVENTS="${FS_EVENTS:+$FS_EVENTS,}cache-misses"

if [ -n "$FS_EVENTS" ]; then
    for readers in 1 2 4 8; do
        echo ""
        echo "  ${readers} reader(s)"
        perf_stat_text "$FS_EVENTS" \
            "$EXE" "--benchmark_filter=FalseSharing/${readers}reader --benchmark_repetitions=1" 3
    done
else
    echo "  WARNING: No cache events available."
fi
echo ""

# ── Memory bandwidth saturation ──────────────────────────────────────────

echo "--- Memory Bandwidth Saturation ---"
BW_EVENTS=""
[ -n "$AVAIL_HW_EVENTS" ] && BW_EVENTS="$AVAIL_HW_EVENTS"
[ -n "$AVAIL_CACHE_EVENTS" ] && BW_EVENTS="${BW_EVENTS:+$BW_EVENTS,}$AVAIL_CACHE_EVENTS"

if [ -n "$BW_EVENTS" ]; then
    perf_stat_text "$BW_EVENTS" \
        "$EXE" "--benchmark_filter=Bandwidth --benchmark_repetitions=1" 3
else
    echo "  WARNING: No hardware events available."
fi

echo ""
echo "=== Memory subsystem analysis complete ==="
echo "Results in: $OUTPUT_DIR/"
