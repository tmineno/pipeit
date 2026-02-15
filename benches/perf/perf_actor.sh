#!/bin/bash
# perf_actor.sh — Actor vectorization and pipeline stall analysis
#
# Group 4: Vectorization effectiveness (IPC as proxy for SIMD utilization)
# Group 5: Pipeline stalls (data dependency stalls, cache misses)

set -e
source "$(dirname "$0")/perf_common.sh"

mkdir -p "$BUILD_DIR" "$OUTPUT_DIR"
trap 'rm -rf "$BUILD_DIR"' EXIT

print_header "Actor Vectorization & Pipeline Stalls"

EXE=$(build_bench "$BENCH_DIR/actor_bench.cpp" "actor_bench")

# ── Vectorization effectiveness ──────────────────────────────────────────
# Higher IPC (instructions/cycle) = better SIMD utilization.

echo "=== Vectorization Effectiveness ==="
echo "(Higher IPC = better SIMD utilization)"
echo ""

VEC_EVENTS=""
probe_event "cpu-cycles" && VEC_EVENTS="cpu-cycles"
probe_event "instructions" && VEC_EVENTS="${VEC_EVENTS:+$VEC_EVENTS,}instructions"
probe_event "stalled-cycles-frontend" && VEC_EVENTS="${VEC_EVENTS:+$VEC_EVENTS,}stalled-cycles-frontend"
[ -n "$AVAIL_BRANCH_EVENTS" ] && VEC_EVENTS="${VEC_EVENTS:+$VEC_EVENTS,}$AVAIL_BRANCH_EVENTS"

if [ -z "$VEC_EVENTS" ]; then
    echo "  WARNING: No hardware events available for vectorization analysis."
else
    # Actors that should vectorize well (simple loops over arrays)
    for actor in "mul" "abs" "mean" "rms" "min" "max" "c2r" "mag"; do
        echo "  Actor: $actor"
        perf_stat_text "$VEC_EVENTS" \
            "$EXE" "--benchmark_filter=Actor_${actor}$ --benchmark_repetitions=1" 5
        echo ""
    done

    # FFT scaling
    echo "  FFT scaling:"
    for n in 64 256 1024 4096; do
        echo "    N=$n"
        perf_stat_text "$VEC_EVENTS" \
            "$EXE" "--benchmark_filter=Actor_fft/${n}$ --benchmark_repetitions=1" 5
        echo ""
    done

    # FIR tap scaling
    echo "  FIR tap scaling:"
    for taps in "5tap" "16tap" "64tap"; do
        echo "    $taps"
        perf_stat_text "$VEC_EVENTS" \
            "$EXE" "--benchmark_filter=Actor_fir_${taps}$ --benchmark_repetitions=1" 5
        echo ""
    done
fi

# ── Pipeline stalls ──────────────────────────────────────────────────────

echo ""
echo "=== Pipeline Stalls ==="
echo "(Data dependencies, cache misses during actor compute)"
echo ""

STALL_EVENTS=""
probe_event "cpu-cycles" && STALL_EVENTS="cpu-cycles"
probe_event "instructions" && STALL_EVENTS="${STALL_EVENTS:+$STALL_EVENTS,}instructions"
probe_event "stalled-cycles-frontend" && STALL_EVENTS="${STALL_EVENTS:+$STALL_EVENTS,}stalled-cycles-frontend"
probe_event "L1-dcache-loads" && STALL_EVENTS="${STALL_EVENTS:+$STALL_EVENTS,}L1-dcache-loads"
probe_event "L1-dcache-load-misses" && STALL_EVENTS="${STALL_EVENTS:+$STALL_EVENTS,}L1-dcache-load-misses"
probe_event "cache-references" && STALL_EVENTS="${STALL_EVENTS:+$STALL_EVENTS,}cache-references"
probe_event "cache-misses" && STALL_EVENTS="${STALL_EVENTS:+$STALL_EVENTS,}cache-misses"

if [ -z "$STALL_EVENTS" ]; then
    echo "  WARNING: No events available for pipeline stall analysis."
else
    # FFT: complex data dependencies (butterfly operations)
    echo "  FFT data dependency analysis:"
    for n in 64 256 1024 4096; do
        echo "    N=$n"
        perf_stat_text "$STALL_EVENTS" \
            "$EXE" "--benchmark_filter=Actor_fft/${n}$ --benchmark_repetitions=1" 5
        echo ""
    done

    # FIR: sequential memory access pattern
    echo "  FIR memory access pattern:"
    for taps in "5tap" "16tap" "64tap"; do
        echo "    $taps"
        perf_stat_text "$STALL_EVENTS" \
            "$EXE" "--benchmark_filter=Actor_fir_${taps}$ --benchmark_repetitions=1" 5
        echo ""
    done
fi

echo "=== Actor analysis complete ==="
echo "Results in: $OUTPUT_DIR/"
