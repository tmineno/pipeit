#!/bin/bash
# Unified benchmark runner for Pipit v0.2.1
#
# Builds and runs all benchmark suites:
#   - Compiler benchmarks (Criterion)
#   - Runtime primitive benchmarks (Google Benchmark)
#   - Ring buffer stress tests
#   - Timer precision benchmarks
#   - Thread scheduling benchmarks
#   - Actor microbenchmarks
#   - End-to-end PDL benchmarks
#   - CPU affinity benchmarks (Google Benchmark)
#   - Memory subsystem benchmarks (Google Benchmark)
#   - Latency breakdown benchmarks (custom)
#   - Perf-based analysis (requires perf)
#
# Usage:
#   ./run_all.sh                    # Run all benchmarks
#   ./run_all.sh --filter compiler  # Run only compiler benchmarks
#   ./run_all.sh --output-dir /tmp/results  # Custom output directory
#   ./run_all.sh --filter pdl --filter actor  # Multiple categories
#   ./run_all.sh --filter perf      # Run perf-based analysis only

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUNTIME_INCLUDE="$PROJECT_ROOT/runtime/libpipit/include"
EXAMPLES_DIR="$PROJECT_ROOT/examples"
BUILD_DIR="/tmp/pipit_bench_build_$$"
OUTPUT_DIR="$SCRIPT_DIR/results"
CXX="${CXX:-c++}"
CXX_FLAGS="-std=c++20 -O3 -march=native -DNDEBUG"

# Detect Google Benchmark library path (prefer /usr/local if present)
BENCH_LIB_FLAGS="-lbenchmark -lpthread"
if [ -f /usr/local/lib/libbenchmark.so ] || [ -f /usr/local/lib/libbenchmark.a ]; then
    BENCH_LIB_FLAGS="-L/usr/local/lib -lbenchmark -lpthread"
fi

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Parse arguments
FILTERS=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --filter)
            FILTERS+=("$2")
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [--filter <category>] [--output-dir <path>]"
            echo ""
            echo "Categories: compiler, runtime, ringbuf, timer, thread, actor, pdl,"
            echo "           affinity, memory, latency, perf, all"
            echo "Default: all"
            exit 0
            ;;
        *)
            echo "Unknown argument: $1"
            exit 1
            ;;
    esac
done

# Default to all if no filters specified
if [ ${#FILTERS[@]} -eq 0 ]; then
    FILTERS=("all")
fi

should_run() {
    local category="$1"
    for f in "${FILTERS[@]}"; do
        if [ "$f" = "all" ] || [ "$f" = "$category" ]; then
            return 0
        fi
    done
    return 1
}

# Setup
mkdir -p "$BUILD_DIR"
mkdir -p "$OUTPUT_DIR"

cleanup() {
    rm -rf "$BUILD_DIR"
}
trap cleanup EXIT

echo -e "${BLUE}=== Pipit Benchmark Suite ===${NC}"
echo "Output directory: $OUTPUT_DIR"
echo "Build directory:  $BUILD_DIR"
echo "Filters:          ${FILTERS[*]}"
echo ""

TOTAL_PASS=0
TOTAL_FAIL=0
TOTAL_SKIP=0

run_section() {
    local name="$1"
    local status="$2"
    if [ "$status" = "pass" ]; then
        TOTAL_PASS=$((TOTAL_PASS + 1))
        echo -e "  ${GREEN}PASS${NC}: $name"
    elif [ "$status" = "fail" ]; then
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
        echo -e "  ${RED}FAIL${NC}: $name"
    else
        TOTAL_SKIP=$((TOTAL_SKIP + 1))
        echo -e "  ${YELLOW}SKIP${NC}: $name"
    fi
}

# ── 1. Compiler benchmarks (Criterion) ──────────────────────────────────────

if should_run "compiler"; then
    echo -e "${GREEN}[1/11] Compiler benchmarks (Criterion)${NC}"
    if cargo bench --manifest-path "$PROJECT_ROOT/compiler/Cargo.toml" \
         -- --output-format bencher 2>&1 | tee "$OUTPUT_DIR/compiler_bench.txt"; then
        run_section "compiler" "pass"
    else
        run_section "compiler" "fail"
    fi
    echo ""
fi

# ── Helper: build and run a C++ benchmark ────────────────────────────────────

build_and_run_gbench() {
    local src="$1"
    local name="$2"
    local extra_flags="${3:-}"
    local exe="$BUILD_DIR/$name"

    echo -e "  Building $name..."
    if ! $CXX $CXX_FLAGS -I "$RUNTIME_INCLUDE" -I "$EXAMPLES_DIR" \
         $extra_flags "$src" $BENCH_LIB_FLAGS -o "$exe" 2>/dev/null; then
        echo -e "  ${YELLOW}Build failed (missing libbenchmark?). Skipping.${NC}"
        return 1
    fi

    echo -e "  Running $name..."
    if "$exe" --benchmark_format=json \
       --benchmark_out="$OUTPUT_DIR/${name}.json" 2>/dev/null; then
        return 0
    else
        return 1
    fi
}

# ── 2. Runtime primitive benchmarks ──────────────────────────────────────────

if should_run "runtime"; then
    echo -e "${GREEN}[2/11] Runtime primitive benchmarks${NC}"
    if build_and_run_gbench "$SCRIPT_DIR/runtime_bench.cpp" "runtime_bench"; then
        run_section "runtime" "pass"
    else
        run_section "runtime" "fail"
    fi
    echo ""
fi

# ── 3. Ring buffer stress tests ──────────────────────────────────────────────

if should_run "ringbuf"; then
    echo -e "${GREEN}[3/11] Ring buffer stress tests${NC}"
    if build_and_run_gbench "$SCRIPT_DIR/ringbuf_bench.cpp" "ringbuf_bench"; then
        run_section "ringbuf" "pass"
    else
        run_section "ringbuf" "fail"
    fi
    echo ""
fi

# ── 4. Timer precision benchmarks ───────────────────────────────────────────

if should_run "timer"; then
    echo -e "${GREEN}[4/11] Timer precision benchmarks${NC}"
    exe="$BUILD_DIR/timer_bench"
    echo -e "  Building timer_bench..."
    if $CXX $CXX_FLAGS -I "$RUNTIME_INCLUDE" \
       "$SCRIPT_DIR/timer_bench.cpp" -lpthread -o "$exe" 2>/dev/null; then
        echo -e "  Running timer_bench..."
        if "$exe" 2>&1 | tee "$OUTPUT_DIR/timer_bench.txt"; then
            run_section "timer" "pass"
        else
            run_section "timer" "fail"
        fi
    else
        echo -e "  ${YELLOW}Build failed. Skipping.${NC}"
        run_section "timer" "fail"
    fi
    echo ""
fi

# ── 5. Thread scheduling benchmarks ─────────────────────────────────────────

if should_run "thread"; then
    echo -e "${GREEN}[5/11] Thread scheduling benchmarks${NC}"
    if build_and_run_gbench "$SCRIPT_DIR/thread_bench.cpp" "thread_bench"; then
        run_section "thread" "pass"
    else
        run_section "thread" "fail"
    fi
    echo ""
fi

# ── 6. Actor microbenchmarks ────────────────────────────────────────────────

if should_run "actor"; then
    echo -e "${GREEN}[6/11] Actor microbenchmarks${NC}"
    if build_and_run_gbench "$SCRIPT_DIR/actor_bench.cpp" "actor_bench"; then
        run_section "actor" "pass"
    else
        run_section "actor" "fail"
    fi
    echo ""
fi

# ── 7. End-to-end PDL benchmarks ────────────────────────────────────────────
#
# Compiles .pdl programs to C++ via pcc, then to native executables.
# Runs each for 1 second with --stats to measure throughput and latency.

if should_run "pdl"; then
    echo -e "${GREEN}[7/11] End-to-end PDL benchmarks${NC}"

    PDL_DIR="$SCRIPT_DIR/pdl"
    PCC="$PROJECT_ROOT/target/release/pcc"
    ACTORS_HEADER="$EXAMPLES_DIR/actors.h"
    PDL_PASS=0
    PDL_FAIL=0

    # Build pcc if needed
    if [ ! -f "$PCC" ]; then
        echo -e "  Building pcc compiler..."
        if ! cargo build --release -p pcc --manifest-path "$PROJECT_ROOT/Cargo.toml" 2>/dev/null; then
            echo -e "  ${YELLOW}pcc build failed. Skipping PDL benchmarks.${NC}"
            run_section "pdl" "fail"
        fi
    fi

    if [ -f "$PCC" ]; then
        {
            echo "=== PDL Runtime Benchmarks ==="
            echo ""

            for pdl in "$PDL_DIR"/*.pdl; do
                [ -f "$pdl" ] || continue
                name="$(basename "$pdl" .pdl)"
                cpp_file="$BUILD_DIR/${name}_generated.cpp"
                exe="$BUILD_DIR/${name}_bench"

                echo "Compiling $name.pdl..."

                # Generate C++ from PDL
                if ! "$PCC" "$pdl" -I "$ACTORS_HEADER" --emit cpp -o "$cpp_file" 2>/dev/null; then
                    echo "  SKIP: $name (pcc compilation failed)"
                    PDL_FAIL=$((PDL_FAIL + 1))
                    continue
                fi

                # Compile to native executable
                if ! $CXX $CXX_FLAGS -I "$RUNTIME_INCLUDE" -I "$EXAMPLES_DIR" \
                     "$cpp_file" -lpthread -o "$exe" 2>/dev/null; then
                    echo "  SKIP: $name (C++ compilation failed)"
                    PDL_FAIL=$((PDL_FAIL + 1))
                    continue
                fi

                # Run for 1 second with stats
                echo "  Running ${name}..."
                "$exe" --duration 1s --stats 2>&1 | grep -E "^\[stats\]|ticks=|avg_latency=" || true
                PDL_PASS=$((PDL_PASS + 1))
                echo ""
            done

            echo "=== PDL Summary ==="
            echo "  Pass: $PDL_PASS  Fail: $PDL_FAIL"
        } 2>&1 | tee "$OUTPUT_DIR/pdl_bench.txt"

        if [ "$PDL_FAIL" -eq 0 ] && [ "$PDL_PASS" -gt 0 ]; then
            run_section "pdl" "pass"
        elif [ "$PDL_PASS" -gt 0 ]; then
            run_section "pdl" "pass"
        else
            run_section "pdl" "fail"
        fi
    fi
    echo ""
fi

# ── 8. CPU affinity benchmarks ─────────────────────────────────────────────

if should_run "affinity"; then
    echo -e "${GREEN}[8/11] CPU affinity benchmarks${NC}"
    if build_and_run_gbench "$SCRIPT_DIR/affinity_bench.cpp" "affinity_bench"; then
        run_section "affinity" "pass"
    else
        run_section "affinity" "fail"
    fi
    echo ""
fi

# ── 9. Memory subsystem benchmarks ────────────────────────────────────────

if should_run "memory"; then
    echo -e "${GREEN}[9/11] Memory subsystem benchmarks${NC}"
    if build_and_run_gbench "$SCRIPT_DIR/memory_bench.cpp" "memory_bench"; then
        run_section "memory" "pass"
    else
        run_section "memory" "fail"
    fi
    echo ""
fi

# ── 10. Latency breakdown benchmarks ─────────────────────────────────────

if should_run "latency"; then
    echo -e "${GREEN}[10/11] Latency breakdown benchmarks${NC}"
    exe="$BUILD_DIR/latency_bench"
    echo -e "  Building latency_bench..."
    if $CXX $CXX_FLAGS -I "$RUNTIME_INCLUDE" -I "$EXAMPLES_DIR" \
       "$SCRIPT_DIR/latency_bench.cpp" -lpthread -o "$exe" 2>/dev/null; then
        echo -e "  Running latency_bench..."
        if "$exe" 2>&1 | tee "$OUTPUT_DIR/latency_bench.txt"; then
            run_section "latency" "pass"
        else
            run_section "latency" "fail"
        fi
    else
        echo -e "  ${YELLOW}Build failed. Skipping.${NC}"
        run_section "latency" "fail"
    fi
    echo ""
fi

# ── 11. Perf-based analysis ───────────────────────────────────────────────

if should_run "perf"; then
    echo -e "${GREEN}[11/11] Perf-based analysis${NC}"
    if command -v perf &>/dev/null; then
        for script in "$SCRIPT_DIR"/perf/perf_*.sh; do
            [ -f "$script" ] || continue
            name="$(basename "$script" .sh)"
            [ "$name" = "perf_common" ] && continue  # Skip shared library
            echo -e "  Running $name..."
            if bash "$script" 2>&1 | tee "$OUTPUT_DIR/${name}_output.txt"; then
                run_section "$name" "pass"
            else
                run_section "$name" "fail"
            fi
        done
    else
        echo -e "  ${YELLOW}perf not found. Skipping perf analysis.${NC}"
        run_section "perf" "skip"
    fi
    echo ""
fi

# ── Summary ──────────────────────────────────────────────────────────────────

echo -e "${BLUE}=== Summary ===${NC}"
echo -e "  Pass: ${GREEN}$TOTAL_PASS${NC}  Fail: ${RED}$TOTAL_FAIL${NC}  Skip: ${YELLOW}$TOTAL_SKIP${NC}"
echo ""
echo "Results saved to: $OUTPUT_DIR/"
ls -la "$OUTPUT_DIR/" 2>/dev/null || true
