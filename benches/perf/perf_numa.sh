#!/bin/bash
# perf_numa.sh — NUMA / CPU topology effects on ring buffer performance
#
# On real NUMA systems: measures cross-socket read/write penalty.
# On single-node (WSL2): measures CPU-distance effects via taskset
# using topology-probed CPU pairs.

set -e
source "$(dirname "$0")/perf_common.sh"

mkdir -p "$BUILD_DIR" "$OUTPUT_DIR"
trap 'rm -rf "$BUILD_DIR"' EXIT

print_header "NUMA / CPU Topology Effects"

EXE=$(build_bench "$BENCH_DIR/ringbuf_bench.cpp" "ringbuf_bench")

EVENTS=""
[ -n "$AVAIL_HW_EVENTS" ] && EVENTS="$AVAIL_HW_EVENTS"
[ -n "$AVAIL_CACHE_EVENTS" ] && EVENTS="${EVENTS:+$EVENTS,}$AVAIL_CACHE_EVENTS"

if [ -z "$EVENTS" ]; then
    echo "WARNING: No hardware/cache events available. Using cpu-clock fallback."
    EVENTS="cpu-clock"
fi

if [ "$NUMA_TOPOLOGY" = "multi" ]; then
    echo "Real NUMA topology detected ($NUMA_NODES nodes)."
    echo ""

    echo "--- Same NUMA node ---"
    numactl --cpunodebind=0 --membind=0 \
        perf stat -r 3 -e "$EVENTS" -- "$EXE" \
        --benchmark_filter=Throughput --benchmark_repetitions=1 \
        2>"$OUTPUT_DIR/perf_numa_same_node.txt"
    cat "$OUTPUT_DIR/perf_numa_same_node.txt"

    echo ""
    echo "--- Cross-node memory ---"
    numactl --cpunodebind=0 --membind=1 \
        perf stat -r 3 -e "$EVENTS" -- "$EXE" \
        --benchmark_filter=Throughput --benchmark_repetitions=1 \
        2>"$OUTPUT_DIR/perf_numa_cross_mem.txt"
    cat "$OUTPUT_DIR/perf_numa_cross_mem.txt"

else
    echo "Single NUMA node detected (WSL2 / single-socket)."
    echo "Running CPU-distance tests via taskset instead."
    echo ""

    echo "--- Same physical core (SMT siblings: CPU $CPU_SMT_A,$CPU_SMT_B) ---"
    taskset -c "$CPU_SMT_A,$CPU_SMT_B" \
        perf stat -r 3 -e "$EVENTS" -- "$EXE" \
        --benchmark_filter=Throughput --benchmark_repetitions=1 \
        2>"$OUTPUT_DIR/perf_numa_smt.txt"
    cat "$OUTPUT_DIR/perf_numa_smt.txt"

    echo ""
    echo "--- Adjacent physical cores (CPU $CPU_NEAR_A,$CPU_NEAR_B) ---"
    taskset -c "$CPU_NEAR_A,$CPU_NEAR_B" \
        perf stat -r 3 -e "$EVENTS" -- "$EXE" \
        --benchmark_filter=Throughput --benchmark_repetitions=1 \
        2>"$OUTPUT_DIR/perf_numa_near.txt"
    cat "$OUTPUT_DIR/perf_numa_near.txt"

    echo ""
    echo "--- Distant physical cores (CPU $CPU_FAR_A,$CPU_FAR_B) ---"
    taskset -c "$CPU_FAR_A,$CPU_FAR_B" \
        perf stat -r 3 -e "$EVENTS" -- "$EXE" \
        --benchmark_filter=Throughput --benchmark_repetitions=1 \
        2>"$OUTPUT_DIR/perf_numa_far.txt"
    cat "$OUTPUT_DIR/perf_numa_far.txt"
fi

echo ""
echo "=== NUMA / topology analysis complete ==="
echo "Results in: $OUTPUT_DIR/"
