#!/bin/bash
# compare_canonical_results.sh - Compare baseline vs current canonical benchmark results.

set -euo pipefail

BASELINE_DIR=""
CURRENT_DIR=""
OUTPUT=""
THRESHOLD_PCT=5
ALLOW_MISSING_BASELINE=false
FAIL_ON_REGRESSION=true

usage() {
    cat <<'EOF'
Usage: compare_canonical_results.sh --baseline-dir <dir> --current-dir <dir> [options]

Options:
  --baseline-dir <dir>        Baseline canonical results directory
  --current-dir <dir>         Current canonical results directory
  --output <file>             Markdown report output path (default: <current-dir>/baseline_comparison.md)
  --threshold-pct <value>     Regression/improvement threshold percentage (default: 5)
  --allow-missing-baseline    Do not fail when baseline file is missing
  --no-fail-on-regression     Report regressions but do not return non-zero
  --help                      Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --baseline-dir)
            BASELINE_DIR="$2"
            shift 2
            ;;
        --current-dir)
            CURRENT_DIR="$2"
            shift 2
            ;;
        --output)
            OUTPUT="$2"
            shift 2
            ;;
        --threshold-pct)
            THRESHOLD_PCT="$2"
            shift 2
            ;;
        --allow-missing-baseline)
            ALLOW_MISSING_BASELINE=true
            shift
            ;;
        --no-fail-on-regression)
            FAIL_ON_REGRESSION=false
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

if [ -z "$BASELINE_DIR" ] || [ -z "$CURRENT_DIR" ]; then
    echo "--baseline-dir and --current-dir are required" >&2
    exit 1
fi
if [ ! -d "$BASELINE_DIR" ]; then
    echo "Baseline directory not found: $BASELINE_DIR" >&2
    exit 1
fi
if [ ! -d "$CURRENT_DIR" ]; then
    echo "Current directory not found: $CURRENT_DIR" >&2
    exit 1
fi
if ! [[ "$THRESHOLD_PCT" =~ ^-?[0-9]+([.][0-9]+)?$ ]]; then
    echo "--threshold-pct must be numeric" >&2
    exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "jq is required" >&2
    exit 1
fi

if [ -z "$OUTPUT" ]; then
    OUTPUT="$CURRENT_DIR/baseline_comparison.md"
fi

TMP_DIR="$(mktemp -d /tmp/pipit_compare_XXXXXX)"
trap 'rm -rf "$TMP_DIR"' EXIT

CURRENT_FILES=()
while IFS= read -r -d '' f; do
    CURRENT_FILES+=("$f")
done < <(find "$CURRENT_DIR" -maxdepth 1 -type f -name '*.canonical.json' -print0 | sort -z)

if [ ${#CURRENT_FILES[@]} -eq 0 ]; then
    echo "No current canonical result files found in $CURRENT_DIR" >&2
    exit 1
fi

MISSING_COUNT=0
TOTAL_MATCHED=0
TOTAL_REGRESSIONS=0
TOTAL_IMPROVEMENTS=0
TOTAL_NEUTRAL=0

MISSING_LIST="$TMP_DIR/missing.txt"
: >"$MISSING_LIST"
FILE_SUMMARY="$TMP_DIR/file_summary.tsv"
: >"$FILE_SUMMARY"
ALL_DIFFS="$TMP_DIR/all_diffs.tsv"
: >"$ALL_DIFFS"

extract_tsv() {
    local input_file="$1"
    local output_tsv="$2"
    jq -r '
      .benchmarks[]? |
      select((.name | type) == "string")
      | select((.cpu_time | type) == "number")
      | select((.time_unit | type) == "string")
      | [ .name, (.cpu_time | tostring), .time_unit ] | @tsv
    ' "$input_file" | sort >"$output_tsv"
}

for current_file in "${CURRENT_FILES[@]}"; do
    base_name="$(basename "$current_file")"
    baseline_file="$BASELINE_DIR/$base_name"

    if [ ! -f "$baseline_file" ]; then
        MISSING_COUNT=$((MISSING_COUNT + 1))
        echo "$base_name" >>"$MISSING_LIST"
        if [ "$ALLOW_MISSING_BASELINE" = false ]; then
            echo "$base_name"$'\t'"0"$'\t'"0"$'\t'"0"$'\t'"0" >>"$FILE_SUMMARY"
        fi
        continue
    fi

    baseline_tsv="$TMP_DIR/${base_name}.baseline.tsv"
    current_tsv="$TMP_DIR/${base_name}.current.tsv"
    diff_tsv="$TMP_DIR/${base_name}.diff.tsv"

    extract_tsv "$baseline_file" "$baseline_tsv"
    extract_tsv "$current_file" "$current_tsv"

    awk -F'\t' -v threshold="$THRESHOLD_PCT" -v fname="$base_name" '
      NR==FNR {
        b[$1] = $2 + 0.0
        u[$1] = $3
        next
      }
      {
        name = $1
        curr = $2 + 0.0
        unit = $3
        if ((name in b) && unit == u[name] && b[name] > 0) {
          pct = ((curr - b[name]) / b[name]) * 100.0
          status = "neutral"
          if (pct > threshold) {
            status = "regression"
          } else if (pct < -threshold) {
            status = "improvement"
          }
          printf "%s\t%s\t%.12g\t%.12g\t%.6f\t%s\t%s\n", fname, name, b[name], curr, pct, status, unit
        }
      }
    ' "$baseline_tsv" "$current_tsv" >"$diff_tsv"

    cat "$diff_tsv" >>"$ALL_DIFFS"

    matched=$(wc -l <"$diff_tsv" | tr -d ' ')
    regressions=$(awk -F'\t' '$6=="regression"{c++} END{print c+0}' "$diff_tsv")
    improvements=$(awk -F'\t' '$6=="improvement"{c++} END{print c+0}' "$diff_tsv")
    neutral=$(awk -F'\t' '$6=="neutral"{c++} END{print c+0}' "$diff_tsv")

    TOTAL_MATCHED=$((TOTAL_MATCHED + matched))
    TOTAL_REGRESSIONS=$((TOTAL_REGRESSIONS + regressions))
    TOTAL_IMPROVEMENTS=$((TOTAL_IMPROVEMENTS + improvements))
    TOTAL_NEUTRAL=$((TOTAL_NEUTRAL + neutral))

    echo "$base_name"$'\t'"$matched"$'\t'"$regressions"$'\t'"$improvements"$'\t'"$neutral" >>"$FILE_SUMMARY"
done

mkdir -p "$(dirname "$OUTPUT")"

{
    echo "# Baseline Comparison Report"
    echo ""
    echo "- Generated: $(date -Iseconds)"
    echo "- Baseline directory: \`$BASELINE_DIR\`"
    echo "- Current directory: \`$CURRENT_DIR\`"
    echo "- Threshold: ${THRESHOLD_PCT}%"
    echo ""

    echo "## Summary"
    echo ""
    echo "| Metric | Value |"
    echo "|---|---:|"
    echo "| Matched benchmark entries | $TOTAL_MATCHED |"
    echo "| Regressions | $TOTAL_REGRESSIONS |"
    echo "| Improvements | $TOTAL_IMPROVEMENTS |"
    echo "| Neutral | $TOTAL_NEUTRAL |"
    echo "| Missing baseline files | $MISSING_COUNT |"
    echo ""

    echo "## Per File"
    echo ""
    echo "| Canonical file | Matched | Regressions | Improvements | Neutral |"
    echo "|---|---:|---:|---:|---:|"
    if [ -s "$FILE_SUMMARY" ]; then
        while IFS=$'\t' read -r file matched reg imp neu; do
            echo "| \`$file\` | $matched | $reg | $imp | $neu |"
        done <"$FILE_SUMMARY"
    fi
    echo ""

    if [ -s "$MISSING_LIST" ]; then
        echo "## Missing Baseline Files"
        echo ""
        while IFS= read -r m; do
            echo "- \`$m\`"
        done <"$MISSING_LIST"
        echo ""
    fi

    echo "## Top Regressions"
    echo ""
    echo "| File | Benchmark | Baseline | Current | Delta % | Unit |"
    echo "|---|---|---:|---:|---:|---|"
    if [ -s "$ALL_DIFFS" ]; then
        awk -F'\t' '$6=="regression"{print}' "$ALL_DIFFS" | sort -t$'\t' -k5,5nr | head -n 30 | \
            while IFS=$'\t' read -r f name b c pct status unit; do
                safe_name="$(printf '%s' "$name" | sed 's/|/\\|/g')"
                printf '| `%s` | %s | %.6g | %.6g | %.3f | %s |\n' "$f" "$safe_name" "$b" "$c" "$pct" "$unit"
            done
    fi
    echo ""
} >"$OUTPUT"

echo "Comparison report generated: $OUTPUT"
echo "Summary: matched=$TOTAL_MATCHED regressions=$TOTAL_REGRESSIONS improvements=$TOTAL_IMPROVEMENTS missing_files=$MISSING_COUNT"

if [ "$ALLOW_MISSING_BASELINE" = false ] && [ "$MISSING_COUNT" -gt 0 ]; then
    exit 1
fi

if [ "$FAIL_ON_REGRESSION" = true ] && [ "$TOTAL_REGRESSIONS" -gt 0 ]; then
    exit 1
fi
