#!/bin/bash
# perf_flamegraph.sh — Generate flame graphs for representative workloads
#
# Requires: FlameGraph tools (auto-downloaded to /tmp if not present)
# Outputs interactive SVG flame graphs to results/ directory.

set -e
source "$(dirname "$0")/perf_common.sh"

mkdir -p "$BUILD_DIR" "$OUTPUT_DIR"
trap 'rm -rf "$BUILD_DIR"' EXIT

print_header "Flame Graph Generation"

# ── Locate or download FlameGraph tools ──────────────────────────────────

FLAMEGRAPH_DIR="/tmp/FlameGraph"
if [ ! -f "$FLAMEGRAPH_DIR/flamegraph.pl" ]; then
    echo "Downloading FlameGraph tools..."
    if git clone --depth 1 https://github.com/brendangregg/FlameGraph.git "$FLAMEGRAPH_DIR" 2>/dev/null; then
        echo "  Downloaded to $FLAMEGRAPH_DIR"
    else
        echo "ERROR: Could not download FlameGraph tools."
        echo "  Install manually: git clone https://github.com/brendangregg/FlameGraph.git $FLAMEGRAPH_DIR"
        exit 1
    fi
fi

STACKCOLLAPSE="$FLAMEGRAPH_DIR/stackcollapse-perf.pl"
FLAMEGRAPH="$FLAMEGRAPH_DIR/flamegraph.pl"

if [ ! -x "$STACKCOLLAPSE" ] || [ ! -x "$FLAMEGRAPH" ]; then
    echo "ERROR: FlameGraph tools not executable at $FLAMEGRAPH_DIR"
    exit 1
fi

# ── Build benchmark binaries ─────────────────────────────────────────────

RINGBUF_EXE=$(build_bench "$BENCH_DIR/ringbuf_bench.cpp" "ringbuf_bench")
ACTOR_EXE=$(build_bench "$BENCH_DIR/actor_bench.cpp" "actor_bench")

# ── Generate flame graphs ────────────────────────────────────────────────

declare -A WORKLOADS
WORKLOADS["ringbuf_throughput"]="$RINGBUF_EXE --benchmark_filter=Throughput --benchmark_repetitions=1"
WORKLOADS["ringbuf_contention"]="$RINGBUF_EXE --benchmark_filter=Contention/8readers --benchmark_repetitions=1"
WORKLOADS["actor_fft_1024"]="$ACTOR_EXE --benchmark_filter=Actor_fft/1024 --benchmark_repetitions=1"
WORKLOADS["actor_fir_64tap"]="$ACTOR_EXE --benchmark_filter=Actor_fir_64tap --benchmark_repetitions=1"

for name in "${!WORKLOADS[@]}"; do
    cmd="${WORKLOADS[$name]}"
    echo "Generating flame graph: $name"

    PERF_DATA="$BUILD_DIR/flamegraph_${name}.data"
    SVG_OUT="$OUTPUT_DIR/flamegraph_${name}.svg"

    # Record with call graph (-F 999 = ~1kHz sampling, -g = call graph)
    if ! perf record -F 999 -g -o "$PERF_DATA" -- $cmd 2>/dev/null; then
        echo "  WARNING: perf record failed for $name (may need elevated privileges)"
        continue
    fi

    # Generate flame graph via pipeline
    if perf script -i "$PERF_DATA" 2>/dev/null | \
       "$STACKCOLLAPSE" 2>/dev/null | \
       "$FLAMEGRAPH" --title "Pipit: $name" --width 1200 > "$SVG_OUT" 2>/dev/null; then
        if [ -s "$SVG_OUT" ]; then
            echo "  Saved: $SVG_OUT"
        else
            echo "  WARNING: Empty flame graph for $name (too few samples?)"
            rm -f "$SVG_OUT"
        fi
    else
        echo "  WARNING: Flame graph generation failed for $name"
    fi

    # Clean up perf.data
    rm -f "$PERF_DATA"
done

echo ""
echo "=== Flame graph generation complete ==="
echo "Open SVG files in a browser for interactive exploration."
echo "Results in: $OUTPUT_DIR/"
