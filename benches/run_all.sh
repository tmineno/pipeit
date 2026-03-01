#!/usr/bin/env bash
# Minimal Pipit benchmark runner.
#
# Features:
#   - Filtered benchmark execution
#   - Report generation
#   - JSON -> Markdown conversion

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUNTIME_INCLUDE="$PROJECT_ROOT/runtime/libpipit/include"
EXAMPLES_DIR="$PROJECT_ROOT/examples"

DEFAULT_BENCH_OUTPUT_DIR="$SCRIPT_DIR/results"
BENCH_OUTPUT_DIR="$DEFAULT_BENCH_OUTPUT_DIR"
REPORT_DIR="$(pwd)"
BUILD_DIR=""
BENCH_CACHE_DIR="$PROJECT_ROOT/target/bench_cache"

CXX="${CXX:-c++}"
CXX_FLAGS="-std=c++20 -O3 -march=native -DNDEBUG"
BENCH_LIB_FLAGS="-lbenchmark -lpthread"
if [ -f /usr/local/lib/libbenchmark.so ] || [ -f /usr/local/lib/libbenchmark.a ]; then
    BENCH_LIB_FLAGS="-L/usr/local/lib -lbenchmark -lpthread"
fi

FILTERS=()
GENERATE_REPORT=false
JSON_INPUT=""
REPORT_TOP=20
QUICK_MODE=false

usage() {
    cat <<'USAGE'
Usage: run_all.sh [options]

Core options:
  --filter <category>      Repeatable. Categories: compiler, ringbuf, timer, thread, pdl, e2e, shm, profile, all
  --output-dir <path>      Output directory for benchmark artifacts and report
  --quick                  Quick mode: shorter runs, skip heavy manual-time benchmarks

Report options:
  --report                 Enable Markdown report generation
  --json <path>            Report source JSON file or directory (JSON -> Markdown mode)
  --report-top <N>         Max rows per benchmark section (default: 20)

Spec examples:
  ./run_all.sh --report
  ./run_all.sh --report --output-dir <path>
  ./run_all.sh --report --json <path> --output-dir <path>
  ./run_all.sh --report --json <path>
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --filter)
            [ $# -ge 2 ] || { echo "--filter requires a value" >&2; exit 1; }
            FILTERS+=("$2")
            shift 2
            ;;
        --output-dir)
            [ $# -ge 2 ] || { echo "--output-dir requires a value" >&2; exit 1; }
            BENCH_OUTPUT_DIR="$2"
            REPORT_DIR="$2"
            shift 2
            ;;
        --report)
            GENERATE_REPORT=true
            shift
            ;;
        --json)
            [ $# -ge 2 ] || { echo "--json requires a value" >&2; exit 1; }
            JSON_INPUT="$2"
            shift 2
            ;;
        --report-top)
            [ $# -ge 2 ] || { echo "--report-top requires a value" >&2; exit 1; }
            REPORT_TOP="$2"
            shift 2
            ;;
        --quick)
            QUICK_MODE=true
            shift
            ;;
        --help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [ ${#FILTERS[@]} -eq 0 ]; then
    FILTERS=("all")
fi

if [ -n "$JSON_INPUT" ] && [ "$GENERATE_REPORT" = false ]; then
    echo "--json requires --report" >&2
    exit 1
fi

if ! [[ "$REPORT_TOP" =~ ^[0-9]+$ ]] || [ "$REPORT_TOP" -le 0 ]; then
    echo "--report-top must be a positive integer" >&2
    exit 1
fi

should_run() {
    local category="$1"
    local f
    for f in "${FILTERS[@]}"; do
        if [ "$f" = "all" ] || [ "$f" = "$category" ]; then
            return 0
        fi
    done
    return 1
}

needs_rebuild() {
    local src="$1"
    local exe="$2"
    [ ! -f "$exe" ] && return 0
    [ "$src" -nt "$exe" ] && return 0
    # Rebuild if any runtime/example header is newer than the binary
    if find "$RUNTIME_INCLUDE" "$EXAMPLES_DIR" -name '*.h' -newer "$exe" -print -quit 2>/dev/null | grep -q .; then
        return 0
    fi
    return 1
}

is_bench_json() {
    local file="$1"
    jq -e 'has("benchmarks") and (.benchmarks | type == "array")' "$file" >/dev/null 2>&1
}

collect_report_files() {
    local files=()
    local file

    if [ -n "$JSON_INPUT" ]; then
        if [ -f "$JSON_INPUT" ]; then
            if is_bench_json "$JSON_INPUT"; then
                files+=("$JSON_INPUT")
            else
                echo "Input is not benchmark JSON: $JSON_INPUT" >&2
                return 1
            fi
        elif [ -d "$JSON_INPUT" ]; then
            while IFS= read -r -d '' file; do
                if is_bench_json "$file"; then
                    files+=("$file")
                fi
            done < <(find "$JSON_INPUT" -maxdepth 1 -type f -name '*.json' -print0 | sort -z)
        else
            echo "--json path not found: $JSON_INPUT" >&2
            return 1
        fi
    else
        if [ ! -d "$BENCH_OUTPUT_DIR" ]; then
            echo "Benchmark output directory not found: $BENCH_OUTPUT_DIR" >&2
            return 1
        fi
        while IFS= read -r -d '' file; do
            if is_bench_json "$file"; then
                files+=("$file")
            fi
        done < <(find "$BENCH_OUTPUT_DIR" -maxdepth 1 -type f -name '*.json' -print0 | sort -z)
    fi

    if [ ${#files[@]} -eq 0 ]; then
        if [ -n "$JSON_INPUT" ]; then
            echo "No benchmark JSON files found from --json: $JSON_INPUT" >&2
        else
            echo "No benchmark JSON files found in: $BENCH_OUTPUT_DIR" >&2
        fi
        return 1
    fi

    printf '%s\n' "${files[@]}"
}

generate_report() {
    if ! command -v jq >/dev/null 2>&1; then
        echo "jq is required for --report" >&2
        return 1
    fi

    mapfile -t files < <(collect_report_files)
    [ ${#files[@]} -gt 0 ] || return 1

    mkdir -p "$REPORT_DIR"
    local report_file="$REPORT_DIR/benchmark_report.md"

    {
        echo "# Benchmark Report"
        echo ""
        echo "- Generated: $(date -Iseconds)"
        if [ -n "$JSON_INPUT" ]; then
            echo "- Source: \`$JSON_INPUT\`"
        else
            echo "- Source directory: \`$BENCH_OUTPUT_DIR\`"
        fi
        echo "- JSON files: ${#files[@]}"
        echo ""
        echo "## Summary"
        echo ""
        echo "| File | Entries | Fastest (CPU) | Slowest (CPU) |"
        echo "|---|---:|---|---|"

        for file in "${files[@]}"; do
            base="$(basename "$file")"
            entries="$(jq -r '[.benchmarks[] | select(has("aggregate_name") | not)] | if length == 0 then (.benchmarks | length) else length end' "$file")"
            fastest="$(jq -r '([.benchmarks[] | select(has("aggregate_name") | not)] | if length == 0 then .benchmarks else . end | min_by(.cpu_time)) as $b | "\($b.name) (\($b.cpu_time) \($b.time_unit))"' "$file")"
            slowest="$(jq -r '([.benchmarks[] | select(has("aggregate_name") | not)] | if length == 0 then .benchmarks else . end | max_by(.cpu_time)) as $b | "\($b.name) (\($b.cpu_time) \($b.time_unit))"' "$file")"
            echo "| \`$base\` | $entries | ${fastest//|/\\|} | ${slowest//|/\\|} |"
        done

        for file in "${files[@]}"; do
            base="$(basename "$file")"
            bench_name="${base%.json}"
            echo ""
            echo "## $bench_name"
            echo ""
            echo "- Date (source): $(jq -r '.context.date // "unknown"' "$file")"
            echo "- CPU: $(jq -r '.context.cpu_info // "unknown"' "$file")"
            echo "- CPUs: $(jq -r '.context.num_cpus // "unknown"' "$file")"
            echo ""
            echo "| Benchmark | CPU time | Real time | Unit | Iterations |"
            echo "|---|---:|---:|---|---:|"

            jq -r --argjson top "$REPORT_TOP" '
              [ .benchmarks[] | select(has("aggregate_name") | not) ]
              | if length == 0 then .benchmarks else . end
              | sort_by(.cpu_time)
              | .[0:$top]
              | .[]
              | [(.name // "-"), (.cpu_time // 0), (.real_time // 0), (.time_unit // "-"), (.iterations // 0)]
              | @tsv
            ' "$file" | while IFS=$'\t' read -r name cpu real unit iter; do
                echo "| ${name//|/\\|} | $cpu | $real | $unit | $iter |"
            done
        done
    } >"$report_file"

    echo "Report generated: $report_file"
}

TOTAL_PASS=0
TOTAL_FAIL=0
TOTAL_SKIP=0
FINAL_EXIT=0

run_section() {
    local name="$1"
    local status="$2"
    case "$status" in
        pass)
            TOTAL_PASS=$((TOTAL_PASS + 1))
            echo "  PASS: $name"
            ;;
        fail)
            TOTAL_FAIL=$((TOTAL_FAIL + 1))
            echo "  FAIL: $name"
            ;;
        *)
            TOTAL_SKIP=$((TOTAL_SKIP + 1))
            echo "  SKIP: $name"
            ;;
    esac
}

# Convert Criterion bencher-format output to Google Benchmark JSON.
# Input: file with lines like "test name ... bench:  1234 ns/iter (+/- 56)"
# Output: Google Benchmark JSON to stdout
bencher_to_gbench_json() {
    local input_file="$1"
    if ! command -v jq >/dev/null 2>&1; then
        # Fallback: raw awk JSON generation
        awk '
        BEGIN { printf "{\n  \"context\": {\"date\": \"%s\", \"library_version\": \"criterion\"},\n  \"benchmarks\": [\n", strftime("%Y-%m-%dT%H:%M:%S%z") }
        /bench:/ {
            # Parse: test <name> ... bench:  <time> ns/iter (+/- <var>)
            name = $2
            for (i = 3; i <= NF; i++) {
                if ($i == "bench:") { time_val = $(i+1); break }
            }
            if (count > 0) printf ",\n"
            printf "    {\"name\": \"%s\", \"cpu_time\": %s, \"real_time\": %s, \"time_unit\": \"ns\", \"iterations\": 0}", name, time_val, time_val
            count++
        }
        END { printf "\n  ]\n}\n" }
        ' "$input_file"
    else
        # Use jq for clean JSON generation
        awk '/bench:/ {
            name = $2
            for (i = 3; i <= NF; i++) {
                if ($i == "bench:") { time_val = $(i+1); break }
            }
            print name "\t" time_val
        }' "$input_file" | jq -Rs --arg date "$(date -Iseconds)" '
            split("\n") | map(select(length > 0) | split("\t") |
                {name: .[0], cpu_time: (.[1] | tonumber), real_time: (.[1] | tonumber),
                 time_unit: "ns", iterations: 0}) |
            {context: {date: $date, library_version: "criterion"}, benchmarks: .}
        '
    fi
}

build_gbench() {
    local src="$1"
    local name="$2"
    shift 2
    local extra_flags="$*"
    local exe="$BENCH_CACHE_DIR/$name"

    if ! needs_rebuild "$src" "$exe"; then
        return 0
    fi

    $CXX $CXX_FLAGS -I "$RUNTIME_INCLUDE" -I "$RUNTIME_INCLUDE/third_party" -I "$EXAMPLES_DIR" \
         "$src" $BENCH_LIB_FLAGS $extra_flags -o "$exe" 2>/dev/null
}

run_gbench() {
    local name="$1"
    local cat="${2:-}"
    local exe="$BENCH_CACHE_DIR/$name"

    local args=("--benchmark_format=json" "--benchmark_out=$BENCH_OUTPUT_DIR/${name}.json")
    if [ "$QUICK_MODE" = true ] && [ -n "$cat" ]; then
        args+=("--benchmark_min_time=0.1s")
        case "$cat" in
            ringbuf) args+=("--benchmark_filter=BM_RingBuffer_Contention") ;;
            timer)   args+=("--benchmark_filter=BM_Timer_FrequencySweep/10000/|BM_Timer_AdaptiveSpin/-1") ;;
            thread)  args+=("--benchmark_filter=BM_TaskDeadline") ;;
            e2e)     args+=("--benchmark_filter=BM_E2E_PipelineOnly") ;;
        esac
    fi

    echo "  Running $name${QUICK_MODE:+ (quick)}..."
    "$exe" "${args[@]}" >/dev/null
}

run_benchmarks() {
    BUILD_DIR="/tmp/pipit_bench_build_$$"
    mkdir -p "$BUILD_DIR"
    mkdir -p "$BENCH_OUTPUT_DIR"
    mkdir -p "$BENCH_CACHE_DIR"

    cleanup() {
        rm -rf "$BUILD_DIR"
    }
    trap cleanup EXIT

    if [ "$QUICK_MODE" = true ]; then
        echo "=== Pipit Benchmark Suite (QUICK MODE) ==="
    else
        echo "=== Pipit Benchmark Suite ==="
    fi
    echo "Output directory: $BENCH_OUTPUT_DIR"
    echo "Build directory:  $BUILD_DIR"
    echo "Filters:          ${FILTERS[*]}"
    echo ""

    if should_run "compiler"; then
        echo "[1/7] Compiler benchmarks"
        local compiler_txt="$BUILD_DIR/compiler_bench_raw.txt"
        if cargo bench --manifest-path "$PROJECT_ROOT/compiler/Cargo.toml" \
            --bench compiler_bench -- --output-format bencher \
            >"$compiler_txt" 2>&1; then
            # Convert bencher text → Google Benchmark JSON for unified reporting
            bencher_to_gbench_json "$compiler_txt" \
                >"$BENCH_OUTPUT_DIR/compiler_bench.json"
            run_section "compiler" "pass"
        else
            run_section "compiler" "fail"
            FINAL_EXIT=1
        fi
        echo ""
    fi

    # ── Parallel C++ benchmark compilation ──
    build_cats=()
    build_pids=()
    for cat in ringbuf timer thread e2e; do
        if should_run "$cat"; then
            build_cats+=("$cat")
            build_gbench "$SCRIPT_DIR/${cat}_bench.cpp" "${cat}_bench" &
            build_pids+=($!)
        fi
    done

    # SHM needs -lrt, build in parallel too
    shm_build_pid=""
    if should_run "shm"; then
        build_gbench "$SCRIPT_DIR/shm_bench.cpp" "shm_bench" -lrt &
        shm_build_pid=$!
    fi

    if [ ${#build_pids[@]} -gt 0 ] || [ -n "$shm_build_pid" ]; then
        echo "[2-6/7] Building C++ benchmarks (parallel, cached)..."
        build_ok=()
        for i in "${!build_pids[@]}"; do
            if wait "${build_pids[$i]}"; then
                build_ok+=("1")
                echo "  ${build_cats[$i]}_bench: OK"
            else
                build_ok+=("0")
                echo "  ${build_cats[$i]}_bench: FAILED"
            fi
        done
        shm_build_ok="0"
        if [ -n "$shm_build_pid" ]; then
            if wait "$shm_build_pid"; then
                shm_build_ok="1"
                echo "  shm_bench: OK"
            else
                echo "  shm_bench: FAILED"
            fi
        fi
        echo ""

        # ── Sequential benchmark execution ──
        idx=0
        for cat in "${build_cats[@]}"; do
            case "$cat" in
                ringbuf) echo "[2/7] Ring buffer benchmarks" ;;
                timer)   echo "[3/7] Timer benchmarks" ;;
                thread)  echo "[4/7] Thread benchmarks" ;;
                e2e)     echo "[5/7] E2E max throughput benchmarks" ;;
            esac
            if [ "${build_ok[$idx]}" = "1" ]; then
                if run_gbench "${cat}_bench" "$cat"; then
                    run_section "$cat" "pass"
                else
                    run_section "$cat" "fail"
                    FINAL_EXIT=1
                fi
            else
                echo "  Build failed"
                run_section "$cat" "fail"
                FINAL_EXIT=1
            fi
            echo ""
            idx=$((idx + 1))
        done
    fi

    if should_run "shm"; then
        echo "[6/7] SHM shared-memory throughput benchmarks"
        if [ "$shm_build_ok" = "1" ]; then
            if run_gbench "shm_bench" "shm"; then
                run_section "shm" "pass"
            else
                run_section "shm" "fail"
                FINAL_EXIT=1
            fi
        else
            echo "  Build failed"
            run_section "shm" "fail"
            FINAL_EXIT=1
        fi
        echo ""
    fi

    if should_run "pdl"; then
        echo "[7/7] End-to-end PDL benchmarks"

        pdl_pass=0
        pdl_fail=0
        pdl_log="$BENCH_OUTPUT_DIR/pdl_bench.txt"

        PCC="$PROJECT_ROOT/target/release/pcc"
        STD_ACTORS_HEADER="$RUNTIME_INCLUDE/std_actors.h"
        STD_MATH_HEADER="$RUNTIME_INCLUDE/std_math.h"
        EXAMPLE_ACTORS_HEADER="$EXAMPLES_DIR/example_actors.h"

        echo "  Building pcc..."
        if ! cargo build --release -p pcc --manifest-path "$PROJECT_ROOT/Cargo.toml" >/dev/null 2>&1; then
            echo "  pcc build failed"
            run_section "pdl" "fail"
            FINAL_EXIT=1
        fi

        if [ -f "$PCC" ]; then
            {
                echo "=== PDL Runtime Benchmarks ==="
                echo ""
                for pdl in "$SCRIPT_DIR/pdl"/*.pdl; do
                    [ -f "$pdl" ] || continue
                    name="$(basename "$pdl" .pdl)"
                    cpp_file="$BUILD_DIR/${name}_generated.cpp"
                    exe="$BUILD_DIR/${name}_bench"

                    echo "Compiling $name.pdl..."
                    if ! "$PCC" "$pdl" -I "$STD_ACTORS_HEADER" -I "$STD_MATH_HEADER" -I "$EXAMPLE_ACTORS_HEADER" --emit cpp -o "$cpp_file" 2>/dev/null; then
                        echo "  SKIP: $name (pcc compilation failed)"
                        pdl_fail=$((pdl_fail + 1))
                        continue
                    fi

                    if ! $CXX $CXX_FLAGS -I "$RUNTIME_INCLUDE" -I "$RUNTIME_INCLUDE/third_party" -I "$EXAMPLES_DIR" "$cpp_file" -lpthread -o "$exe" 2>/dev/null; then
                        echo "  SKIP: $name (C++ compilation failed)"
                        pdl_fail=$((pdl_fail + 1))
                        continue
                    fi

                    echo "  Running $name..."
                    pdl_stderr_log="$BUILD_DIR/${name}_runtime.stderr"
                    if "$exe" --duration 1s --stats > /dev/null 2>"$pdl_stderr_log"; then
                        grep -E '^\[stats\]|ticks=|avg_latency=' "$pdl_stderr_log" || true
                        pdl_pass=$((pdl_pass + 1))
                    else
                        echo "  FAIL: $name (runtime exited non-zero)"
                        cat "$pdl_stderr_log"
                        pdl_fail=$((pdl_fail + 1))
                    fi
                    echo ""
                done
                echo "=== PDL Summary ==="
                echo "  Pass: $pdl_pass  Fail: $pdl_fail"
            } > >(tee "$pdl_log") 2>&1

            if [ "$pdl_pass" -gt 0 ] && [ "$pdl_fail" -eq 0 ]; then
                run_section "pdl" "pass"
            elif [ "$pdl_pass" -gt 0 ]; then
                run_section "pdl" "fail"
                FINAL_EXIT=1
            else
                run_section "pdl" "fail"
                FINAL_EXIT=1
            fi
        fi

        echo ""
    fi

    if should_run "profile"; then
        echo "[6/6] Block profile benchmarks (uftrace)"
        if command -v uftrace >/dev/null 2>&1; then
            if "$SCRIPT_DIR/profile_bench.sh" \
                --output-dir "$BENCH_OUTPUT_DIR/profile" \
                --duration 2s \
                >"$BENCH_OUTPUT_DIR/profile_bench.txt" 2>&1; then
                run_section "profile" "pass"
            else
                run_section "profile" "fail"
                FINAL_EXIT=1
            fi
        else
            echo "  uftrace not installed, skipping"
            run_section "profile" "skip"
        fi
        echo ""
    fi

    echo "=== Summary ==="
    echo "  Pass: $TOTAL_PASS  Fail: $TOTAL_FAIL  Skip: $TOTAL_SKIP"
    echo ""
    echo "Results saved to: $BENCH_OUTPUT_DIR"
    ls -la "$BENCH_OUTPUT_DIR" 2>/dev/null || true
}

# --json 指定時は変換専用モード（ベンチは実行しない）
if [ -z "$JSON_INPUT" ]; then
    run_benchmarks
fi

if [ "$GENERATE_REPORT" = true ]; then
    echo ""
    echo "=== Report Generation ==="
    if ! generate_report; then
        FINAL_EXIT=1
    fi
fi

exit "$FINAL_EXIT"
