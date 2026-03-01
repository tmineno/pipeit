#!/usr/bin/env bash
# A/B profiling script for Pipit compiler optimizations.
#
# Runs N independent compiler benchmark invocations on a baseline ref and
# current HEAD, then computes median/p90/stddev and writes machine-readable
# csv/json artifacts.
#
# Usage:
#   ./benches/profile_ab.sh --baseline-ref <git-ref> [options]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
STABLE_BENCH="$SCRIPT_DIR/compiler_bench_stable.sh"

# Defaults (per profiling_protocol.md)
N=10
CPU="${PIPIT_BENCH_CPU:-1}"
SAMPLE_SIZE=40
MEASUREMENT_TIME=1.0
WARMUP_TIME=0.2
BENCH_FILTER="kpi/"
BASELINE_REF=""
OUTPUT_DIR="$PROJECT_ROOT/doc/performance"
VERSION_TAG="v046"

usage() {
    cat <<'USAGE'
Usage: profile_ab.sh --baseline-ref <ref> [options]

Options:
  --baseline-ref <ref>   Git ref for baseline (required)
  --n <count>            Number of independent runs (default: 10)
  --cpu <id>             CPU core for taskset (default: 1 or $PIPIT_BENCH_CPU)
  --filter <pattern>     Criterion filter (default: kpi/)
  --version-tag <tag>    Tag for output files (default: v046)
  --output-dir <path>    Output directory (default: doc/performance/)
  --help                 Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --baseline-ref) BASELINE_REF="$2"; shift 2 ;;
        --n) N="$2"; shift 2 ;;
        --cpu) CPU="$2"; shift 2 ;;
        --filter) BENCH_FILTER="$2"; shift 2 ;;
        --version-tag) VERSION_TAG="$2"; shift 2 ;;
        --output-dir) OUTPUT_DIR="$2"; shift 2 ;;
        --help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
    esac
done

if [[ -z "$BASELINE_REF" ]]; then
    echo "ERROR: --baseline-ref is required" >&2
    usage >&2
    exit 1
fi

log() { echo "[profile_ab] $*"; }

# ── Helpers ──────────────────────────────────────────────────────────────

run_n_times() {
    local label="$1"
    local repo_root="$2"
    local results_file="$3"
    shift 3

    log "Running $N invocations for $label ..."
    for i in $(seq 1 "$N"); do
        log "  invocation $i/$N"
        (
            cd "$repo_root"
            export CARGO_TARGET_DIR="$PROJECT_ROOT/target/stable_bench"
            taskset -c "$CPU" cargo bench \
                --manifest-path compiler/Cargo.toml \
                --bench compiler_bench \
                -- "$BENCH_FILTER" \
                --sample-size "$SAMPLE_SIZE" \
                --measurement-time "$MEASUREMENT_TIME" \
                --warm-up-time "$WARMUP_TIME" \
                --output-format bencher 2>/dev/null
        ) >> "$results_file"
        echo "---RUN_BOUNDARY---" >> "$results_file"
    done
}

extract_medians() {
    # Parse bencher-format output: "test <name> ... bench: <ns> ns/iter (+/- <ns>)"
    local input_file="$1"
    grep '^test ' "$input_file" | \
        sed -E 's/^test ([^ ]+) +\.\.\. bench: +([0-9,]+) ns\/iter.*/\1 \2/' | \
        tr -d ','
}

compute_stats() {
    # Given a file with "benchmark_name value" lines (multiple runs),
    # compute median, p90, stddev per benchmark and output CSV.
    local input_file="$1"
    awk '
    {
        name = $1
        value = $2 + 0
        data[name][++count[name]] = value
        sum[name] += value
        sumsq[name] += value * value
    }
    END {
        for (name in count) {
            n = count[name]
            # Sort values for percentiles
            for (i = 1; i <= n; i++)
                sorted[i] = data[name][i]
            for (i = 1; i <= n; i++)
                for (j = i+1; j <= n; j++)
                    if (sorted[j] < sorted[i]) {
                        t = sorted[i]; sorted[i] = sorted[j]; sorted[j] = t
                    }
            median = (n % 2 == 1) ? sorted[int(n/2)+1] : (sorted[n/2] + sorted[n/2+1]) / 2
            p90_idx = int(n * 0.9 + 0.5)
            if (p90_idx < 1) p90_idx = 1
            if (p90_idx > n) p90_idx = n
            p90 = sorted[p90_idx]
            mean = sum[name] / n
            variance = (n > 1) ? (sumsq[name] / n - mean * mean) : 0
            stddev = (variance > 0) ? sqrt(variance) : 0
            printf "%s,%.1f,%.1f,%.1f\n", name, median, p90, stddev
        }
    }
    ' "$input_file" | sort
}

# ── Main ─────────────────────────────────────────────────────────────────

TMP_DIR="$(mktemp -d /tmp/pipit-profile-ab.XXXXXX)"
cleanup() {
    git -C "$PROJECT_ROOT" worktree remove "$TMP_DIR/baseline_wt" >/dev/null 2>&1 || true
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

BASELINE_RAW="$TMP_DIR/baseline_raw.txt"
CURRENT_RAW="$TMP_DIR/current_raw.txt"
BASELINE_MEDIANS="$TMP_DIR/baseline_medians.txt"
CURRENT_MEDIANS="$TMP_DIR/current_medians.txt"

# Set up baseline worktree
log "Creating baseline worktree at $BASELINE_REF ..."
git -C "$PROJECT_ROOT" worktree add "$TMP_DIR/baseline_wt" "$BASELINE_REF" >/dev/null

# Run baseline
run_n_times "baseline ($BASELINE_REF)" "$TMP_DIR/baseline_wt" "$BASELINE_RAW"

# Run current
run_n_times "current (HEAD)" "$PROJECT_ROOT" "$CURRENT_RAW"

# Extract and compute stats
extract_medians "$BASELINE_RAW" > "$BASELINE_MEDIANS"
extract_medians "$CURRENT_RAW" > "$CURRENT_MEDIANS"

BASELINE_STATS="$TMP_DIR/baseline_stats.csv"
CURRENT_STATS="$TMP_DIR/current_stats.csv"
compute_stats "$BASELINE_MEDIANS" > "$BASELINE_STATS"
compute_stats "$CURRENT_MEDIANS" > "$CURRENT_STATS"

# ── Write CSV ────────────────────────────────────────────────────────────

CSV_OUT="$OUTPUT_DIR/${VERSION_TAG}_ab_comparison.csv"
mkdir -p "$OUTPUT_DIR"

{
    echo "benchmark,baseline_median_ns,baseline_p90_ns,baseline_stddev_ns,current_median_ns,current_p90_ns,current_stddev_ns,delta_pct"
    join -t',' "$BASELINE_STATS" "$CURRENT_STATS" | while IFS=',' read -r name b_med b_p90 b_std c_med c_p90 c_std; do
        if [ "$b_med" != "0" ] && [ "$b_med" != "0.0" ]; then
            delta=$(awk "BEGIN { printf \"%.2f\", (($c_med - $b_med) / $b_med) * 100 }")
        else
            delta="NA"
        fi
        echo "$name,$b_med,$b_p90,$b_std,$c_med,$c_p90,$c_std,$delta"
    done
} > "$CSV_OUT"

log "CSV written: $CSV_OUT"

# ── Write JSON ───────────────────────────────────────────────────────────

JSON_OUT="$OUTPUT_DIR/${VERSION_TAG}_ab_comparison.json"
{
    echo "{"
    echo "  \"baseline_ref\": \"$BASELINE_REF\","
    echo "  \"n_runs\": $N,"
    echo "  \"sample_size\": $SAMPLE_SIZE,"
    echo "  \"measurement_time_s\": $MEASUREMENT_TIME,"
    echo "  \"cpu_pin\": $CPU,"
    echo "  \"benchmarks\": ["
    first=true
    while IFS=',' read -r name b_med b_p90 b_std c_med c_p90 c_std delta; do
        [ "$name" = "benchmark" ] && continue
        if [ "$first" = true ]; then first=false; else echo ","; fi
        printf '    {"name":"%s","baseline":{"median_ns":%s,"p90_ns":%s,"stddev_ns":%s},"current":{"median_ns":%s,"p90_ns":%s,"stddev_ns":%s},"delta_pct":%s}' \
            "$name" "$b_med" "$b_p90" "$b_std" "$c_med" "$c_p90" "$c_std" "${delta:-null}"
    done < "$CSV_OUT"
    echo ""
    echo "  ]"
    echo "}"
} > "$JSON_OUT"

log "JSON written: $JSON_OUT"

# ── Gate checks ──────────────────────────────────────────────────────────

REGRESSION_FAIL=false
IMPROVED_COUNT=0
GATE_PHASES="full_compile_latency/complex full_compile_latency/modal phase_latency/schedule/complex phase_latency/codegen/complex"

log ""
log "=== Gate Verdict ==="

while IFS=',' read -r name b_med b_p90 b_std c_med c_p90 c_std delta; do
    [ "$name" = "benchmark" ] && continue
    for phase in $GATE_PHASES; do
        if echo "$name" | grep -q "$phase"; then
            if [ "$delta" != "NA" ]; then
                is_regression=$(awk "BEGIN { print ($delta > 5.0) ? 1 : 0 }")
                is_improvement=$(awk "BEGIN { print ($delta < 0) ? 1 : 0 }")
                if [ "$is_regression" = "1" ]; then
                    log "REGRESSION: $name delta=${delta}% (>5% threshold)"
                    REGRESSION_FAIL=true
                fi
                if [ "$is_improvement" = "1" ]; then
                    IMPROVED_COUNT=$((IMPROVED_COUNT + 1))
                    log "IMPROVED: $name delta=${delta}%"
                fi
            fi
            break
        fi
    done
done < "$CSV_OUT"

log ""
if [ "$REGRESSION_FAIL" = true ]; then
    log "REGRESSION GATE: FAIL (one or more phases regressed >5%)"
else
    log "REGRESSION GATE: PASS"
fi

if [ "$IMPROVED_COUNT" -ge 2 ]; then
    log "IMPROVEMENT GATE: PASS ($IMPROVED_COUNT workloads improved)"
else
    log "IMPROVEMENT GATE: FAIL (only $IMPROVED_COUNT/2 workloads improved)"
fi

log ""
log "Artifacts:"
log "  CSV:  $CSV_OUT"
log "  JSON: $JSON_OUT"

if [ "$REGRESSION_FAIL" = true ]; then
    exit 1
fi
