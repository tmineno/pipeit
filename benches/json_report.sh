#!/bin/bash
# json_report.sh - Generate human-readable Markdown report from benchmark JSON files.
#
# Supports Google Benchmark JSON files with:
#   {
#     "context": { ... },
#     "benchmarks": [ ... ]
#   }
#
# Usage examples:
#   ./json_report.sh --input-dir ./results
#   ./json_report.sh --input-dir ./results --bench actor_bench --bench thread_bench
#   ./json_report.sh --input-dir ./results --output ./results/my_report.md --top 30

set -euo pipefail

INPUT_DIR=""
OUTPUT=""
TOP_N=20
BENCHES=()

usage() {
    cat <<'EOF'
Usage: json_report.sh --input-dir <dir> [--output <file>] [--bench <name>] [--top <N>]

Options:
  --input-dir <dir>   Directory that contains benchmark JSON files
  --output <file>     Output Markdown file (default: <input-dir>/benchmark_report.md)
  --bench <name>      Selected bench name/file; repeatable (e.g. actor_bench)
  --top <N>           Max detailed rows per bench (default: 20)
  --help              Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --input-dir)
            if [ $# -lt 2 ]; then
                echo "--input-dir requires a value" >&2
                exit 1
            fi
            INPUT_DIR="$2"
            shift 2
            ;;
        --output)
            if [ $# -lt 2 ]; then
                echo "--output requires a value" >&2
                exit 1
            fi
            OUTPUT="$2"
            shift 2
            ;;
        --bench)
            if [ $# -lt 2 ]; then
                echo "--bench requires a value" >&2
                exit 1
            fi
            BENCHES+=("$2")
            shift 2
            ;;
        --top)
            if [ $# -lt 2 ]; then
                echo "--top requires a value" >&2
                exit 1
            fi
            TOP_N="$2"
            shift 2
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

if [ -z "$INPUT_DIR" ]; then
    echo "--input-dir is required" >&2
    usage >&2
    exit 1
fi

if [ ! -d "$INPUT_DIR" ]; then
    echo "Input directory not found: $INPUT_DIR" >&2
    exit 1
fi

if ! [[ "$TOP_N" =~ ^[0-9]+$ ]] || [ "$TOP_N" -le 0 ]; then
    echo "--top must be a positive integer" >&2
    exit 1
fi

if [ -z "$OUTPUT" ]; then
    OUTPUT="$INPUT_DIR/benchmark_report.md"
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "jq is required to generate report from JSON files" >&2
    exit 1
fi

is_gbench_json() {
    local file="$1"
    jq -e 'has("benchmarks") and (.benchmarks | type == "array")' "$file" >/dev/null 2>&1
}

resolve_bench_file() {
    local spec="$1"
    if [ -f "$spec" ]; then
        printf '%s\n' "$spec"
        return 0
    fi
    if [ -f "$INPUT_DIR/$spec" ]; then
        printf '%s\n' "$INPUT_DIR/$spec"
        return 0
    fi
    if [ -f "$INPUT_DIR/$spec.json" ]; then
        printf '%s\n' "$INPUT_DIR/$spec.json"
        return 0
    fi
    return 1
}

FILES=()
if [ ${#BENCHES[@]} -gt 0 ]; then
    for bench in "${BENCHES[@]}"; do
        if file=$(resolve_bench_file "$bench"); then
            if is_gbench_json "$file"; then
                FILES+=("$file")
            else
                echo "Skipping non-Google-Benchmark JSON: $file" >&2
            fi
        else
            echo "Warning: bench JSON not found for selection '$bench'" >&2
        fi
    done
else
    while IFS= read -r -d '' file; do
        if is_gbench_json "$file"; then
            FILES+=("$file")
        fi
    done < <(find "$INPUT_DIR" -maxdepth 1 -type f -name '*.json' -print0 | sort -z)
fi

if [ ${#FILES[@]} -eq 0 ]; then
    echo "No Google Benchmark JSON files found for report generation" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"

escape_md() {
    sed 's/|/\\|/g'
}

{
    echo "# Benchmark Report"
    echo ""
    echo "- Generated: $(date -Iseconds)"
    echo "- Input directory: \`$INPUT_DIR\`"
    echo "- Selected files: ${#FILES[@]}"
    echo "- Max rows per bench section: $TOP_N"
    echo ""

    echo "## Summary"
    echo ""
    echo "| Bench file | Entries | Fastest (CPU) | Slowest (CPU) |"
    echo "|---|---:|---|---|"

    for file in "${FILES[@]}"; do
        base="$(basename "$file")"
        entries="$(jq -r '[.benchmarks[] | select(has("aggregate_name") | not)] | length' "$file")"
        if [ "$entries" -eq 0 ]; then
            entries="$(jq -r '.benchmarks | length' "$file")"
            fastest="$(jq -r '.benchmarks | min_by(.cpu_time) | "\(.name) (\(.cpu_time) \(.time_unit))"' "$file")"
            slowest="$(jq -r '.benchmarks | max_by(.cpu_time) | "\(.name) (\(.cpu_time) \(.time_unit))"' "$file")"
        else
            fastest="$(jq -r '[.benchmarks[] | select(has("aggregate_name") | not)] | min_by(.cpu_time) | "\(.name) (\(.cpu_time) \(.time_unit))"' "$file")"
            slowest="$(jq -r '[.benchmarks[] | select(has("aggregate_name") | not)] | max_by(.cpu_time) | "\(.name) (\(.cpu_time) \(.time_unit))"' "$file")"
        fi
        fastest="$(printf '%s' "$fastest" | escape_md)"
        slowest="$(printf '%s' "$slowest" | escape_md)"
        echo "| \`$base\` | $entries | $fastest | $slowest |"
    done

    for file in "${FILES[@]}"; do
        base="$(basename "$file")"
        bench_name="${base%.json}"

        echo ""
        echo "## $bench_name"
        echo ""

        cpu_info="$(jq -r '.context.cpu_info // "unknown"' "$file")"
        num_cpus="$(jq -r '.context.num_cpus // "unknown"' "$file")"
        mhz_per_cpu="$(jq -r '.context.mhz_per_cpu // "unknown"' "$file")"
        date_utc="$(jq -r '.context.date // "unknown"' "$file")"

        echo "- Date (source): $date_utc"
        echo "- CPU: $cpu_info"
        echo "- CPUs: $num_cpus"
        echo "- MHz per CPU: $mhz_per_cpu"
        echo ""
        echo "| Benchmark | CPU time | Real time | Unit | Iterations |"
        echo "|---|---:|---:|---|---:|"

        jq -r --argjson top "$TOP_N" '
          [ .benchmarks[] | select(has("aggregate_name") | not) ]
          | if length == 0 then [ .benchmarks[] ] else . end
          | sort_by(.cpu_time)
          | .[0:$top]
          | .[]
          | [
              (.name // "-"),
              (.cpu_time // 0 | tostring),
              (.real_time // 0 | tostring),
              (.time_unit // "-"),
              (.iterations // 0 | tostring)
            ]
          | @tsv
        ' "$file" | while IFS=$'\t' read -r bname cpu_t real_t unit iter; do
            bname="$(printf '%s' "$bname" | escape_md)"
            echo "| $bname | $cpu_t | $real_t | $unit | $iter |"
        done
    done
} >"$OUTPUT"

echo "Report generated: $OUTPUT"
