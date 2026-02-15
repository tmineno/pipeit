#!/bin/bash
# Unified benchmark runner for Pipit v0.2.1
#
# Builds and runs all benchmark suites:
#   - Compiler benchmarks (Criterion)
#   - Runtime primitive benchmarks (Google Benchmark)
#   - Ring buffer stress tests
#   - Timer precision benchmarks (Google Benchmark)
#   - Thread scheduling benchmarks
#   - Actor microbenchmarks
#   - End-to-end PDL benchmarks
#   - CPU affinity benchmarks (Google Benchmark)
#   - Memory subsystem benchmarks (Google Benchmark)
#   - Latency breakdown benchmarks (Google Benchmark)
#   - Perf-based analysis (requires perf)
#
# Usage:
#   ./run_all.sh                    # Run all benchmarks
#   ./run_all.sh --filter compiler  # Run only compiler benchmarks
#   ./run_all.sh --output-dir /tmp/results  # Custom output directory
#   ./run_all.sh --filter pdl --filter actor  # Multiple categories
#   ./run_all.sh --filter perf      # Run perf-based analysis only
#   ./run_all.sh --report           # Generate human-readable Markdown report
#   ./run_all.sh --report --report-bench actor_bench --report-bench thread_bench
#   ./run_all.sh --validate         # Validate canonical JSON outputs
#   ./run_all.sh --compare-baseline-dir ./benches/baselines/nightly

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUNTIME_INCLUDE="$PROJECT_ROOT/runtime/libpipit/include"
EXAMPLES_DIR="$PROJECT_ROOT/examples"
BUILD_DIR="/tmp/pipit_bench_build_$$"
OUTPUT_DIR="$SCRIPT_DIR/results"
CANONICALIZER="$SCRIPT_DIR/canonicalize_results.sh"
VALIDATOR="$SCRIPT_DIR/validate_canonical_results.sh"
COMPARATOR="$SCRIPT_DIR/compare_canonical_results.sh"
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
GENERATE_REPORT=false
REPORT_BENCHES=()
REPORT_OUTPUT=""
REPORT_TOP=20
VALIDATE_CANONICAL=false
VALIDATE_SCHEMA="$SCRIPT_DIR/schema/canonical-benchmark.schema.json"
COMPARE_BASELINE_DIR=""
COMPARE_OUTPUT=""
COMPARE_THRESHOLD_PCT=5
COMPARE_ALLOW_MISSING_BASELINE=false
COMPARE_FAIL_ON_REGRESSION=true
while [[ $# -gt 0 ]]; do
    case "$1" in
        --filter)
            if [ $# -lt 2 ]; then
                echo "--filter requires a value"
                exit 1
            fi
            FILTERS+=("$2")
            shift 2
            ;;
        --output-dir)
            if [ $# -lt 2 ]; then
                echo "--output-dir requires a value"
                exit 1
            fi
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --report)
            GENERATE_REPORT=true
            shift
            ;;
        --report-bench)
            if [ $# -lt 2 ]; then
                echo "--report-bench requires a value"
                exit 1
            fi
            REPORT_BENCHES+=("$2")
            shift 2
            ;;
        --report-output)
            if [ $# -lt 2 ]; then
                echo "--report-output requires a value"
                exit 1
            fi
            REPORT_OUTPUT="$2"
            shift 2
            ;;
        --report-top)
            if [ $# -lt 2 ]; then
                echo "--report-top requires a value"
                exit 1
            fi
            REPORT_TOP="$2"
            shift 2
            ;;
        --validate)
            VALIDATE_CANONICAL=true
            shift
            ;;
        --validate-schema)
            if [ $# -lt 2 ]; then
                echo "--validate-schema requires a value"
                exit 1
            fi
            VALIDATE_SCHEMA="$2"
            shift 2
            ;;
        --compare-baseline-dir)
            if [ $# -lt 2 ]; then
                echo "--compare-baseline-dir requires a value"
                exit 1
            fi
            COMPARE_BASELINE_DIR="$2"
            shift 2
            ;;
        --compare-output)
            if [ $# -lt 2 ]; then
                echo "--compare-output requires a value"
                exit 1
            fi
            COMPARE_OUTPUT="$2"
            shift 2
            ;;
        --compare-threshold-pct)
            if [ $# -lt 2 ]; then
                echo "--compare-threshold-pct requires a value"
                exit 1
            fi
            COMPARE_THRESHOLD_PCT="$2"
            shift 2
            ;;
        --compare-allow-missing-baseline)
            COMPARE_ALLOW_MISSING_BASELINE=true
            shift
            ;;
        --compare-no-fail-on-regression)
            COMPARE_FAIL_ON_REGRESSION=false
            shift
            ;;
        --help)
            echo "Usage: $0 [--filter <category>] [--output-dir <path>] [options]"
            echo ""
            echo "Categories: compiler, runtime, ringbuf, timer, thread, actor, pdl,"
            echo "           affinity, memory, latency, perf, all"
            echo ""
            echo "Report options:"
            echo "  --report                      Generate Markdown report from benchmark JSON files"
            echo "  --report-bench <name>         Include only selected bench JSON(s), e.g. actor_bench"
            echo "  --report-output <path>        Report file path (default: <output-dir>/benchmark_report.md)"
            echo "  --report-top <N>              Max rows per bench section (default: 20)"
            echo ""
            echo "Validation options:"
            echo "  --validate                    Validate canonical JSON artifacts after run"
            echo "  --validate-schema <path>      Schema path (default: benches/schema/canonical-benchmark.schema.json)"
            echo ""
            echo "Baseline comparison options:"
            echo "  --compare-baseline-dir <dir>  Compare against canonical JSON baseline directory"
            echo "  --compare-output <file>       Comparison report output (default: <output-dir>/baseline_comparison.md)"
            echo "  --compare-threshold-pct <N>   Regression threshold percent (default: 5)"
            echo "  --compare-allow-missing-baseline"
            echo "                                Ignore missing baseline files"
            echo "  --compare-no-fail-on-regression"
            echo "                                Report regressions without non-zero exit"
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

if [ -z "$REPORT_OUTPUT" ]; then
    REPORT_OUTPUT="$OUTPUT_DIR/benchmark_report.md"
fi
if [ -z "$COMPARE_OUTPUT" ]; then
    COMPARE_OUTPUT="$OUTPUT_DIR/baseline_comparison.md"
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
GENERATED_JSON_BENCHES=()
CANONICAL_OUTPUTS=()
CANONICAL_FAIL=0
FINAL_EXIT=0

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

canonicalize_from_input() {
    local kind="$1"
    local suite="$2"
    local input_file="$3"
    local output_file="$OUTPUT_DIR/${suite}.canonical.json"
    if ! bash "$CANONICALIZER" --kind "$kind" --suite "$suite" \
         --input "$input_file" --output "$output_file" >/dev/null 2>&1; then
        echo -e "  ${YELLOW}WARN${NC}: canonicalization failed for $suite"
        CANONICAL_FAIL=$((CANONICAL_FAIL + 1))
        return 1
    fi
    CANONICAL_OUTPUTS+=("$output_file")
    return 0
}

canonicalize_perf_dir() {
    local suite="$1"
    local output_file="$OUTPUT_DIR/${suite}.canonical.json"
    if ! bash "$CANONICALIZER" --kind perf --suite "$suite" \
         --results-dir "$OUTPUT_DIR" --output "$output_file" >/dev/null 2>&1; then
        echo -e "  ${YELLOW}WARN${NC}: canonicalization failed for $suite"
        CANONICAL_FAIL=$((CANONICAL_FAIL + 1))
        return 1
    fi
    CANONICAL_OUTPUTS+=("$output_file")
    return 0
}

# ── 1. Compiler benchmarks (Criterion) ──────────────────────────────────────

if should_run "compiler"; then
    echo -e "${GREEN}[1/11] Compiler benchmarks (Criterion)${NC}"
    if cargo bench --manifest-path "$PROJECT_ROOT/compiler/Cargo.toml" \
         --bench compiler_bench -- --output-format bencher 2>&1 | tee "$OUTPUT_DIR/compiler_bench.txt"; then
        run_section "compiler" "pass"
    else
        run_section "compiler" "fail"
    fi
    canonicalize_from_input "compiler" "compiler" "$OUTPUT_DIR/compiler_bench.txt" || true
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
        GENERATED_JSON_BENCHES+=("$name")
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
    canonicalize_from_input "gbench" "runtime" "$OUTPUT_DIR/runtime_bench.json" || true
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
    canonicalize_from_input "gbench" "ringbuf" "$OUTPUT_DIR/ringbuf_bench.json" || true
    echo ""
fi

# ── 4. Timer precision benchmarks ───────────────────────────────────────────

if should_run "timer"; then
    echo -e "${GREEN}[4/11] Timer precision benchmarks${NC}"
    if build_and_run_gbench "$SCRIPT_DIR/timer_bench.cpp" "timer_bench"; then
        run_section "timer" "pass"
    else
        run_section "timer" "fail"
    fi
    canonicalize_from_input "gbench" "timer" "$OUTPUT_DIR/timer_bench.json" || true
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
    canonicalize_from_input "gbench" "thread" "$OUTPUT_DIR/thread_bench.json" || true
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
    canonicalize_from_input "gbench" "actor" "$OUTPUT_DIR/actor_bench.json" || true
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
    canonicalize_from_input "pdl" "pdl" "$OUTPUT_DIR/pdl_bench.txt" || true
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
    canonicalize_from_input "gbench" "affinity" "$OUTPUT_DIR/affinity_bench.json" || true
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
    canonicalize_from_input "gbench" "memory" "$OUTPUT_DIR/memory_bench.json" || true
    echo ""
fi

# ── 10. Latency breakdown benchmarks ─────────────────────────────────────

if should_run "latency"; then
    echo -e "${GREEN}[10/11] Latency breakdown benchmarks${NC}"
    if build_and_run_gbench "$SCRIPT_DIR/latency_bench.cpp" "latency_bench"; then
        run_section "latency" "pass"
    else
        run_section "latency" "fail"
    fi
    canonicalize_from_input "gbench" "latency" "$OUTPUT_DIR/latency_bench.json" || true
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
            if OUTPUT_DIR="$OUTPUT_DIR" bash "$script" 2>&1 | tee "$OUTPUT_DIR/${name}_output.txt"; then
                run_section "$name" "pass"
            else
                run_section "$name" "fail"
            fi
        done
    else
        echo -e "  ${YELLOW}perf not found. Skipping perf analysis.${NC}"
        run_section "perf" "skip"
    fi
    canonicalize_perf_dir "perf" || true
    echo ""
fi

# ── Summary ──────────────────────────────────────────────────────────────────

echo -e "${BLUE}=== Summary ===${NC}"
echo -e "  Pass: ${GREEN}$TOTAL_PASS${NC}  Fail: ${RED}$TOTAL_FAIL${NC}  Skip: ${YELLOW}$TOTAL_SKIP${NC}"
echo ""
echo "Results saved to: $OUTPUT_DIR/"
ls -la "$OUTPUT_DIR/" 2>/dev/null || true
if [ ${#CANONICAL_OUTPUTS[@]} -gt 0 ]; then
    echo ""
    echo "Canonical JSON artifacts:"
    for f in "${CANONICAL_OUTPUTS[@]}"; do
        echo "  - $f"
    done
fi
if [ "$CANONICAL_FAIL" -gt 0 ]; then
    echo -e "${YELLOW}Canonicalization warnings: $CANONICAL_FAIL${NC}"
fi

if [ "$GENERATE_REPORT" = true ]; then
    echo ""
    echo -e "${BLUE}=== Human-Readable Report ===${NC}"
    REPORT_CMD=(bash "$SCRIPT_DIR/json_report.sh" --input-dir "$OUTPUT_DIR" --output "$REPORT_OUTPUT" --top "$REPORT_TOP")
    REPORT_SELECTION=("${REPORT_BENCHES[@]}")
    if [ ${#REPORT_SELECTION[@]} -eq 0 ] && [ ${#CANONICAL_OUTPUTS[@]} -gt 0 ]; then
        REPORT_SELECTION=()
        for canonical_path in "${CANONICAL_OUTPUTS[@]}"; do
            canonical_base="$(basename "$canonical_path" .json)"
            REPORT_SELECTION+=("$canonical_base")
        done
    elif [ ${#REPORT_SELECTION[@]} -eq 0 ] && [ ${#GENERATED_JSON_BENCHES[@]} -gt 0 ]; then
        REPORT_SELECTION=("${GENERATED_JSON_BENCHES[@]}")
    fi
    for bench_name in "${REPORT_SELECTION[@]}"; do
        REPORT_CMD+=(--bench "$bench_name")
    done
    if "${REPORT_CMD[@]}"; then
        echo -e "  ${GREEN}PASS${NC}: report generated at $REPORT_OUTPUT"
    else
        echo -e "  ${RED}FAIL${NC}: report generation failed"
    fi
fi

if [ "$VALIDATE_CANONICAL" = true ]; then
    echo ""
    echo -e "${BLUE}=== Canonical Validation ===${NC}"
    VALIDATE_CMD=(bash "$VALIDATOR" --schema "$VALIDATE_SCHEMA")
    if [ ${#CANONICAL_OUTPUTS[@]} -gt 0 ]; then
        for canonical_path in "${CANONICAL_OUTPUTS[@]}"; do
            VALIDATE_CMD+=(--file "$canonical_path")
        done
    else
        VALIDATE_CMD+=(--input-dir "$OUTPUT_DIR")
    fi
    if "${VALIDATE_CMD[@]}"; then
        echo -e "  ${GREEN}PASS${NC}: canonical validation passed"
    else
        echo -e "  ${RED}FAIL${NC}: canonical validation failed"
        FINAL_EXIT=1
    fi
fi

if [ -n "$COMPARE_BASELINE_DIR" ]; then
    echo ""
    echo -e "${BLUE}=== Baseline Comparison ===${NC}"
    COMPARE_CMD=(
        bash "$COMPARATOR"
        --baseline-dir "$COMPARE_BASELINE_DIR"
        --current-dir "$OUTPUT_DIR"
        --threshold-pct "$COMPARE_THRESHOLD_PCT"
        --output "$COMPARE_OUTPUT"
    )
    if [ "$COMPARE_ALLOW_MISSING_BASELINE" = true ]; then
        COMPARE_CMD+=(--allow-missing-baseline)
    fi
    if [ "$COMPARE_FAIL_ON_REGRESSION" = false ]; then
        COMPARE_CMD+=(--no-fail-on-regression)
    fi
    if "${COMPARE_CMD[@]}"; then
        echo -e "  ${GREEN}PASS${NC}: comparison report generated at $COMPARE_OUTPUT"
    else
        echo -e "  ${RED}FAIL${NC}: baseline comparison failed"
        FINAL_EXIT=1
    fi
fi

exit "$FINAL_EXIT"
