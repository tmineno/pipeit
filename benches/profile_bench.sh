#!/usr/bin/env bash
# Profile generated code blocks using uftrace.
#
# Compiles each PDL in benches/pdl/ with -finstrument-functions and runs
# uftrace to produce per-function timing reports and flame graph data.
#
# Requirements:
#   - uftrace (https://github.com/namhyung/uftrace)
#   - Rust toolchain (cargo) for building pcc
#
# Usage:
#   ./profile_bench.sh                        # profile all PDLs
#   ./profile_bench.sh benches/pdl/simple.pdl  # profile specific PDL
#   ./profile_bench.sh --duration 5s           # custom run duration
#   ./profile_bench.sh --output-dir /tmp/prof  # custom output directory

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUNTIME_INCLUDE="$PROJECT_ROOT/runtime/libpipit/include"
EXAMPLES_DIR="$PROJECT_ROOT/examples"

DEFAULT_OUTPUT_DIR="$SCRIPT_DIR/results/profile"
OUTPUT_DIR="$DEFAULT_OUTPUT_DIR"
DURATION="2s"
PDL_FILES=()

CXX="${CXX:-c++}"
# -pg omitted: uftrace works with -finstrument-functions directly.
# -O2 keeps optimizations while still instrumenting function boundaries.
PROFILE_CXX_FLAGS="-std=c++20 -O2 -g -finstrument-functions"

usage() {
    cat <<'USAGE'
Usage: profile_bench.sh [options] [pdl_files...]

Options:
  --duration <dur>       Run duration per PDL (default: 2s)
  --output-dir <path>    Output directory for reports (default: benches/results/profile)
  --help                 Show this help

If no PDL files are specified, all benches/pdl/*.pdl are profiled.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --duration)
            [ $# -ge 2 ] || { echo "--duration requires a value" >&2; exit 1; }
            DURATION="$2"
            shift 2
            ;;
        --output-dir)
            [ $# -ge 2 ] || { echo "--output-dir requires a value" >&2; exit 1; }
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --help)
            usage
            exit 0
            ;;
        -*)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
        *)
            PDL_FILES+=("$1")
            shift
            ;;
    esac
done

# ── Dependency checks ─────────────────────────────────────────────────────

if ! command -v uftrace >/dev/null 2>&1; then
    echo "ERROR: uftrace is not installed." >&2
    echo "" >&2
    echo "Install with:" >&2
    echo "  Ubuntu/Debian: sudo apt install uftrace" >&2
    echo "  Fedora:        sudo dnf install uftrace" >&2
    echo "  From source:   https://github.com/namhyung/uftrace" >&2
    exit 1
fi

# ── Build pcc ──────────────────────────────────────────────────────────────

PCC="$PROJECT_ROOT/target/release/pcc"
STD_ACTORS_HEADER="$RUNTIME_INCLUDE/std_actors.h"
EXAMPLE_ACTORS_HEADER="$EXAMPLES_DIR/example_actors.h"

echo "=== Pipit Block Profiler (uftrace) ==="
echo ""
echo "Building pcc..."
if ! cargo build --release -p pcc --manifest-path "$PROJECT_ROOT/Cargo.toml" 2>&1 | tail -1; then
    echo "ERROR: pcc build failed" >&2
    exit 1
fi

if [ ! -f "$PCC" ]; then
    echo "ERROR: pcc binary not found at $PCC" >&2
    exit 1
fi

# ── Resolve PDL files ──────────────────────────────────────────────────────

if [ ${#PDL_FILES[@]} -eq 0 ]; then
    for pdl in "$SCRIPT_DIR/pdl"/*.pdl; do
        [ -f "$pdl" ] && PDL_FILES+=("$pdl")
    done
fi

if [ ${#PDL_FILES[@]} -eq 0 ]; then
    echo "No PDL files found to profile." >&2
    exit 1
fi

# ── Setup directories ─────────────────────────────────────────────────────

BUILD_DIR="/tmp/pipit_profile_build_$$"
mkdir -p "$BUILD_DIR"
mkdir -p "$OUTPUT_DIR"

cleanup() {
    rm -rf "$BUILD_DIR"
}
trap cleanup EXIT

# ── Profile each PDL ──────────────────────────────────────────────────────

TOTAL=0
PASS=0
FAIL=0

for pdl in "${PDL_FILES[@]}"; do
    name="$(basename "$pdl" .pdl)"
    TOTAL=$((TOTAL + 1))
    cpp_file="$BUILD_DIR/${name}_generated.cpp"
    exe="$BUILD_DIR/${name}_profiled"
    trace_dir="$BUILD_DIR/${name}.uftrace"
    report_file="$OUTPUT_DIR/${name}_report.txt"
    flamegraph_data="$OUTPUT_DIR/${name}_flamegraph.txt"

    echo ""
    echo "--- $name.pdl ---"

    # Step 1: PDL → C++
    echo "  [1/4] Compiling PDL → C++..."
    if ! "$PCC" "$pdl" -I "$STD_ACTORS_HEADER" -I "$EXAMPLE_ACTORS_HEADER" --emit cpp -o "$cpp_file" 2>/dev/null; then
        echo "  SKIP: pcc compilation failed"
        FAIL=$((FAIL + 1))
        continue
    fi

    # Step 2: C++ → instrumented binary
    echo "  [2/4] Compiling C++ with -finstrument-functions..."
    if ! $CXX $PROFILE_CXX_FLAGS -I "$RUNTIME_INCLUDE" -I "$EXAMPLES_DIR" \
         "$cpp_file" -lpthread -o "$exe" 2>&1; then
        echo "  SKIP: C++ compilation failed"
        FAIL=$((FAIL + 1))
        continue
    fi

    # Step 3: Run with uftrace
    echo "  [3/4] Recording with uftrace (duration=$DURATION)..."
    if ! uftrace record \
         --no-pager \
         -d "$trace_dir" \
         "$exe" --duration "$DURATION" > /dev/null 2>&1; then
        echo "  SKIP: uftrace record failed (runtime may have exited non-zero)"
        FAIL=$((FAIL + 1))
        continue
    fi

    # Step 4: Generate reports
    echo "  [4/4] Generating reports..."

    # Function report sorted by total time
    {
        echo "# Profile Report: $name"
        echo "# Generated: $(date -Iseconds)"
        echo "# Duration: $DURATION"
        echo "# Source: $pdl"
        echo "#"
        echo ""
        uftrace report -d "$trace_dir" --sort total 2>/dev/null
    } > "$report_file"

    # Flame graph data (folded stacks format)
    # Note: uftrace dump --flame-graph may crash on some versions; non-critical.
    uftrace dump -d "$trace_dir" --flame-graph > "$flamegraph_data" 2>/dev/null &
    if ! wait $! 2>/dev/null; then
        rm -f "$flamegraph_data"
    fi

    # Generate SVG if flamegraph.pl is available and flame data exists
    if [ -s "$flamegraph_data" ] && command -v flamegraph.pl >/dev/null 2>&1; then
        flamegraph.pl "$flamegraph_data" > "$OUTPUT_DIR/${name}_flamegraph.svg" 2>/dev/null || true
        echo "  Flame graph: $OUTPUT_DIR/${name}_flamegraph.svg"
    fi

    # Print top-10 summary to stdout
    echo ""
    echo "  Top functions by total time:"
    uftrace report -d "$trace_dir" --sort total 2>/dev/null | head -14
    echo ""
    echo "  Full report: $report_file"

    PASS=$((PASS + 1))
done

# ── Summary ────────────────────────────────────────────────────────────────

echo ""
echo "=== Profile Summary ==="
echo "  Total: $TOTAL  Pass: $PASS  Fail: $FAIL"
echo "  Results: $OUTPUT_DIR"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
