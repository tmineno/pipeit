#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUN_ALL_SCRIPT="$SCRIPT_DIR/run_all.sh"
REPORT_ROOT="$PROJECT_ROOT/doc/performance"
TMP_ROOT="$PROJECT_ROOT/tmp/performance"

COMMIT_ID=""
ID_SOURCE="head"
SKIP_IF_EXISTS=false
FORCE=false
STRICT=true

COMPILE_TIMEOUT_SEC=30
COMPILE_SAMPLE_SIZE=10
COMPILE_MEASUREMENT_TIME=0.10
COMPILE_WARMUP_TIME=0.05

usage() {
    cat <<'USAGE'
Usage: commit_characterize.sh [options]

Runs runtime/e2e + bounded compile benchmarks and writes:
  doc/performance/<id>-characterize.md

Options:
  --commit-id <id>               Explicit report id (overrides --id-source)
  --id-source <head|staged-diff> ID source when --commit-id is not given
                                 head:        git rev-parse --short HEAD
                                 staged-diff: hash of staged diff excluding doc/performance reports
  --skip-if-exists               Skip run if target report already exists
  --force                        Overwrite existing report
  --no-strict                    Generate report even when benchmark command fails (exit 0)
  --compile-timeout-sec <n>      Compile benchmark timeout seconds (default: 30)
  --compile-sample-size <n>      Criterion sample size (default: 10)
  --compile-measurement-time <s> Criterion measurement time in seconds (default: 0.10)
  --compile-warm-up-time <s>     Criterion warm-up time in seconds (default: 0.05)
  --help                         Show this help

Examples:
  ./benches/commit_characterize.sh
  ./benches/commit_characterize.sh --id-source staged-diff --skip-if-exists
  ./benches/commit_characterize.sh --commit-id 1a2b3c4d --force
USAGE
}

log() {
    echo "[characterize] $*"
}

warn() {
    echo "[characterize][warn] $*" >&2
}

is_number() {
    [[ "$1" =~ ^-?[0-9]+([.][0-9]+)?([eE][-+]?[0-9]+)?$ ]]
}

format_si() {
    local value="$1"
    local show_plus="${2:-false}"
    if [ "$value" = "NA" ]; then
        echo "NA"
        return
    fi
    if ! is_number "$value"; then
        echo "$value"
        return
    fi
    awk -v x="$value" -v plus="$show_plus" '
        function absv(v) { return (v < 0) ? -v : v }
        function trim_zeros(s) {
            if (index(s, ".") > 0) {
                sub(/0+$/, "", s)
                sub(/\.$/, "", s)
            }
            return s
        }
        BEGIN {
            if (x == 0) {
                if (plus == "true") {
                    print "+0"
                } else {
                    print "0"
                }
                exit
            }

            sign = ""
            if (x < 0) {
                sign = "-"
            } else if (plus == "true") {
                sign = "+"
            }

            a = absv(x)
            exp3 = 0
            if (a >= 1000) {
                while (a >= 1000 && exp3 < 15) {
                    a /= 1000
                    exp3 += 3
                }
            }

            prefix = ""
            if (exp3 == 3) prefix = "K"
            else if (exp3 == 6) prefix = "M"
            else if (exp3 == 9) prefix = "G"
            else if (exp3 == 12) prefix = "T"
            else if (exp3 == 15) prefix = "P"

            if (a >= 100) {
                num = sprintf("%.0f", a)
            } else if (a >= 10) {
                num = sprintf("%.1f", a)
            } else if (a >= 1) {
                num = sprintf("%.2f", a)
            } else {
                num = sprintf("%.4f", a)
            }
            num = trim_zeros(num)
            printf "%s%s%s", sign, num, prefix
        }
    '
}

compute_staged_diff_id() {
    local diff_hash
    diff_hash="$(git -C "$PROJECT_ROOT" diff --cached --binary -- . ':(exclude)doc/performance/*-bench.md' | sha1sum | awk '{print $1}')"
    if [ "$diff_hash" = "da39a3ee5e6b4b0d3255bfef95601890afd80709" ]; then
        git -C "$PROJECT_ROOT" rev-parse --short=7 HEAD
    else
        echo "${diff_hash:0:12}"
    fi
}

json_value() {
    local json_file="$1"
    local bench_name="$2"
    local field="$3"
    if [ ! -f "$json_file" ]; then
        echo "NA"
        return
    fi
    local value
    value="$(jq -r --arg n "$bench_name" --arg f "$field" '
        [
          .benchmarks[]
          | select(.name == $n and ((.error_occurred // false) | not))
          | .[$f]
        ]
        | first // empty
    ' "$json_file" 2>/dev/null || true)"
    if [ -z "$value" ] || [ "$value" = "null" ]; then
        echo "NA"
    else
        echo "$value"
    fi
}

metric_delta() {
    local current="$1"
    local previous="$2"
    if [ "$current" = "NA" ] || [ "$previous" = "NA" ] || [ -z "$previous" ]; then
        echo "-"
        return
    fi
    if ! is_number "$current" || ! is_number "$previous"; then
        echo "-"
        return
    fi
    local delta
    delta="$(awk -v c="$current" -v p="$previous" 'BEGIN { printf "%.17g", c - p }')"
    local delta_fmt
    delta_fmt="$(format_si "$delta" true)"
    local is_prev_zero
    is_prev_zero="$(awk -v p="$previous" 'BEGIN { if (p == 0) print 1; else print 0 }')"
    if [ "$is_prev_zero" -eq 1 ]; then
        echo "$delta_fmt"
    else
        local pct
        pct="$(awk -v c="$current" -v p="$previous" 'BEGIN { printf "%+.2f%%", ((c - p) / p) * 100.0 }')"
        echo "$delta_fmt ($pct)"
    fi
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --commit-id)
            [ $# -ge 2 ] || { echo "--commit-id requires a value" >&2; exit 1; }
            COMMIT_ID="$2"
            shift 2
            ;;
        --id-source)
            [ $# -ge 2 ] || { echo "--id-source requires a value" >&2; exit 1; }
            ID_SOURCE="$2"
            shift 2
            ;;
        --skip-if-exists)
            SKIP_IF_EXISTS=true
            shift
            ;;
        --force)
            FORCE=true
            shift
            ;;
        --no-strict)
            STRICT=false
            shift
            ;;
        --compile-timeout-sec)
            [ $# -ge 2 ] || { echo "--compile-timeout-sec requires a value" >&2; exit 1; }
            COMPILE_TIMEOUT_SEC="$2"
            shift 2
            ;;
        --compile-sample-size)
            [ $# -ge 2 ] || { echo "--compile-sample-size requires a value" >&2; exit 1; }
            COMPILE_SAMPLE_SIZE="$2"
            shift 2
            ;;
        --compile-measurement-time)
            [ $# -ge 2 ] || { echo "--compile-measurement-time requires a value" >&2; exit 1; }
            COMPILE_MEASUREMENT_TIME="$2"
            shift 2
            ;;
        --compile-warm-up-time)
            [ $# -ge 2 ] || { echo "--compile-warm-up-time requires a value" >&2; exit 1; }
            COMPILE_WARMUP_TIME="$2"
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

case "$ID_SOURCE" in
    head|staged-diff) ;;
    *)
        echo "--id-source must be one of: head, staged-diff" >&2
        exit 1
        ;;
esac

# Skip if HEAD commit does not touch Rust source in compiler/.
# pre-commit post-commit stage provides no file list, so we check here.
compiler_rs_changed="$(git -C "$PROJECT_ROOT" diff --name-only HEAD~1 HEAD -- 'compiler/src/*.rs' 'compiler/tests/*.rs' 2>/dev/null || true)"
if [ -z "$compiler_rs_changed" ]; then
    log "no compiler Rust source changed in HEAD — skipping"
    exit 0
fi

for cmd in git cargo jq timeout sha1sum sed awk; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Missing required command: $cmd" >&2
        exit 1
    fi
done

if [ -z "$COMMIT_ID" ]; then
    if [ "$ID_SOURCE" = "staged-diff" ]; then
        COMMIT_ID="$(compute_staged_diff_id)"
    else
        COMMIT_ID="$(git -C "$PROJECT_ROOT" rev-parse --short=7 HEAD)"
    fi
fi

mkdir -p "$REPORT_ROOT"

REPORT_TIMESTAMP="$(date +%Y%m%dT%H%M%S)"
REPORT_FILE="$REPORT_ROOT/${REPORT_TIMESTAMP}-${COMMIT_ID}-bench.md"

# skip-if-exists: check for any existing report with the same commit ID
if [ "$SKIP_IF_EXISTS" = true ] && [ "$FORCE" = false ]; then
    existing="$(compgen -G "$REPORT_ROOT/*-${COMMIT_ID}-bench.md" 2>/dev/null | head -1 || true)"
    if [ -n "$existing" ]; then
        log "report already exists, skipping: $existing"
        git -C "$PROJECT_ROOT" add "$existing"
        exit 0
    fi
fi

RUN_ID="$(date +%Y%m%d-%H%M%S)"
RUN_DIR="$TMP_ROOT/${COMMIT_ID}-${RUN_ID}"
RUNTIME_OUT_DIR="$RUN_DIR/runtime"
RUNTIME_LOG="$RUN_DIR/runtime.log"
COMPILE_LOG="$RUN_DIR/compile.log"
REPORT_TMP="$RUN_DIR/report.md"
RUN_DIR_REL="tmp/performance/${COMMIT_ID}-${RUN_ID}"
RUNTIME_OUT_DIR_REL="${RUN_DIR_REL}/runtime"
RUNTIME_LOG_REL="${RUN_DIR_REL}/runtime.log"
COMPILE_LOG_REL="${RUN_DIR_REL}/compile.log"

mkdir -p "$RUN_DIR"

runtime_cmd=(
    "$RUN_ALL_SCRIPT"
    --filter ringbuf
    --filter timer
    --filter thread
    --filter e2e
    --output-dir "$RUNTIME_OUT_DIR"
)

compile_cmd=(
    cargo bench
    --manifest-path "$PROJECT_ROOT/compiler/Cargo.toml"
    --bench compiler_bench
    -- kpi/full_compile_latency
    --sample-size "$COMPILE_SAMPLE_SIZE"
    --measurement-time "$COMPILE_MEASUREMENT_TIME"
    --warm-up-time "$COMPILE_WARMUP_TIME"
    --output-format bencher
)
runtime_cmd_display="benches/run_all.sh --filter ringbuf --filter timer --filter thread --filter e2e --output-dir ${RUNTIME_OUT_DIR_REL}"
compile_cmd_display="cargo bench --manifest-path compiler/Cargo.toml --bench compiler_bench -- kpi/full_compile_latency --sample-size ${COMPILE_SAMPLE_SIZE} --measurement-time ${COMPILE_MEASUREMENT_TIME} --warm-up-time ${COMPILE_WARMUP_TIME} --output-format bencher"

log "report id: $COMMIT_ID (source=$ID_SOURCE)"
log "running runtime/e2e benchmarks..."
set +e
"${runtime_cmd[@]}" >"$RUNTIME_LOG" 2>&1
runtime_status=$?
set -e

compile_start_ns="$(date +%s%N)"
log "running compile benchmark (timeout ${COMPILE_TIMEOUT_SEC}s)..."
set +e
timeout "${COMPILE_TIMEOUT_SEC}s" "${compile_cmd[@]}" >"$COMPILE_LOG" 2>&1
compile_status=$?
set -e
compile_end_ns="$(date +%s%N)"
compile_wall_ms="$(awk -v s="$compile_start_ns" -v e="$compile_end_ns" 'BEGIN {printf "%.3f", (e - s) / 1000000.0}')"

compile_timed_out=0
if [ "$compile_status" -eq 124 ]; then
    compile_timed_out=1
fi

ringbuf_json="$RUNTIME_OUT_DIR/ringbuf_bench.json"
timer_json="$RUNTIME_OUT_DIR/timer_bench.json"
thread_json="$RUNTIME_OUT_DIR/thread_bench.json"
e2e_json="$RUNTIME_OUT_DIR/e2e_bench.json"

declare -A compile_ns=(
    [simple]="NA"
    [multitask]="NA"
    [complex]="NA"
    [modal]="NA"
)

if [ -f "$COMPILE_LOG" ]; then
    while read -r scenario ns; do
        if [[ -n "${compile_ns[$scenario]+x}" ]]; then
            compile_ns["$scenario"]="$ns"
        fi
    done < <(sed -nE 's/^test kpi\/full_compile_latency\/([^ ]+) .* bench:[[:space:]]*([0-9]+) ns\/iter.*/\1 \2/p' "$COMPILE_LOG")
fi

socket_error_count="NA"
socket_error_message="-"
if [ -f "$e2e_json" ]; then
    socket_error_count="$(jq -r '[.benchmarks[] | select((.name | startswith("BM_E2E_SocketLoopback/")) and (.error_occurred // false))] | length' "$e2e_json")"
    socket_error_message="$(jq -r '[.benchmarks[] | select((.name | startswith("BM_E2E_SocketLoopback/")) and (.error_occurred // false)) | .error_message] | unique | join("; ")' "$e2e_json")"
    if [ -z "$socket_error_message" ] || [ "$socket_error_message" = "null" ]; then
        socket_error_message="-"
    fi
fi

branch_name="$(git -C "$PROJECT_ROOT" rev-parse --abbrev-ref HEAD)"
head_commit="$(git -C "$PROJECT_ROOT" rev-parse --short HEAD)"

declare -A metric_values
declare -A metric_units
metric_keys=()

add_metric() {
    local key="$1"
    local value="$2"
    local unit="$3"
    metric_keys+=("$key")
    metric_values["$key"]="$value"
    metric_units["$key"]="$unit"
}

add_metric "compile.full.simple_ns_per_iter" "${compile_ns[simple]}" "ns/iter"
add_metric "compile.full.multitask_ns_per_iter" "${compile_ns[multitask]}" "ns/iter"
add_metric "compile.full.complex_ns_per_iter" "${compile_ns[complex]}" "ns/iter"
add_metric "compile.full.modal_ns_per_iter" "${compile_ns[modal]}" "ns/iter"
add_metric "compile.full.wall_ms" "$compile_wall_ms" "ms"
add_metric "compile.full.timed_out" "$compile_timed_out" "bool(0/1)"

add_metric "runtime.thread.deadline_1khz_miss_rate_pct" \
    "$(json_value "$thread_json" "BM_TaskDeadline/1000/2000/iterations:1/manual_time" "miss_rate_pct")" \
    "pct"
add_metric "runtime.thread.deadline_10khz_miss_rate_pct" \
    "$(json_value "$thread_json" "BM_TaskDeadline/10000/3000/iterations:1/manual_time" "miss_rate_pct")" \
    "pct"
add_metric "runtime.thread.deadline_48khz_miss_rate_pct" \
    "$(json_value "$thread_json" "BM_TaskDeadline/48000/3000/iterations:1/manual_time" "miss_rate_pct")" \
    "pct"

add_metric "runtime.timer.freq_10khz_p99_ns" \
    "$(json_value "$timer_json" "BM_Timer_FrequencySweep/10000/2000/iterations:1/manual_time" "p99_ns")" \
    "ns"
add_metric "runtime.timer.freq_10khz_overruns" \
    "$(json_value "$timer_json" "BM_Timer_FrequencySweep/10000/2000/iterations:1/manual_time" "overruns")" \
    "count"
add_metric "runtime.timer.adaptive_auto_p99_ns" \
    "$(json_value "$timer_json" "BM_Timer_AdaptiveSpin/-1/iterations:1/manual_time" "p99_ns")" \
    "ns"

add_metric "runtime.ringbuf.contention_1reader_tokens_per_sec" \
    "$(json_value "$ringbuf_json" "BM_RingBuffer_Contention/1reader" "reader_tokens_per_sec")" \
    "items/s"
add_metric "runtime.ringbuf.contention_2readers_tokens_per_sec" \
    "$(json_value "$ringbuf_json" "BM_RingBuffer_Contention/2readers" "reader_tokens_per_sec")" \
    "items/s"
add_metric "runtime.ringbuf.contention_4readers_tokens_per_sec" \
    "$(json_value "$ringbuf_json" "BM_RingBuffer_Contention/4readers" "reader_tokens_per_sec")" \
    "items/s"
add_metric "runtime.ringbuf.contention_8readers_tokens_per_sec" \
    "$(json_value "$ringbuf_json" "BM_RingBuffer_Contention/8readers" "reader_tokens_per_sec")" \
    "items/s"
add_metric "runtime.ringbuf.contention_4readers_read_fail_pct" \
    "$(json_value "$ringbuf_json" "BM_RingBuffer_Contention/4readers" "read_fail_pct")" \
    "pct"

add_metric "e2e.pipeline_64_samples_per_sec" \
    "$(json_value "$e2e_json" "BM_E2E_PipelineOnly/64" "items_per_second")" \
    "samples/s"
add_metric "e2e.pipeline_256_samples_per_sec" \
    "$(json_value "$e2e_json" "BM_E2E_PipelineOnly/256" "items_per_second")" \
    "samples/s"
add_metric "e2e.pipeline_1024_samples_per_sec" \
    "$(json_value "$e2e_json" "BM_E2E_PipelineOnly/1024" "items_per_second")" \
    "samples/s"

add_metric "e2e.socket_64_rx_samples_per_sec" \
    "$(json_value "$e2e_json" "BM_E2E_SocketLoopback/64/iterations:1/manual_time" "rx_samples_per_sec")" \
    "samples/s"
add_metric "e2e.socket_256_rx_samples_per_sec" \
    "$(json_value "$e2e_json" "BM_E2E_SocketLoopback/256/iterations:1/manual_time" "rx_samples_per_sec")" \
    "samples/s"
add_metric "e2e.socket_1024_rx_samples_per_sec" \
    "$(json_value "$e2e_json" "BM_E2E_SocketLoopback/1024/iterations:1/manual_time" "rx_samples_per_sec")" \
    "samples/s"
add_metric "e2e.socket_error_count" "$socket_error_count" "count"

previous_report=""
if compgen -G "$REPORT_ROOT/*-bench.md" >/dev/null 2>&1; then
    while IFS= read -r candidate; do
        if [ "$candidate" != "$REPORT_FILE" ]; then
            previous_report="$candidate"
            break
        fi
    done < <(ls -1t "$REPORT_ROOT"/*-bench.md)
fi

declare -A prev_values
declare -A prev_units
if [ -n "$previous_report" ] && [ -f "$previous_report" ]; then
    while IFS='|' read -r key value unit; do
        [ -n "$key" ] || continue
        prev_values["$key"]="$value"
        prev_units["$key"]="$unit"
    done < <(awk '/<!-- PIPIT_METRICS_BEGIN -->/{flag=1; next} /<!-- PIPIT_METRICS_END -->/{flag=0} flag {print}' "$previous_report")
fi

{
    echo "# Commit Characterization"
    echo ""
    echo "- ID: \`$COMMIT_ID\` (source: \`$ID_SOURCE\`)"
    echo "- HEAD: \`$head_commit\`"
    echo "- Branch: \`$branch_name\`"
    echo "- Generated: $(date -Iseconds)"
    if [ -n "$previous_report" ]; then
        echo "- Previous report: \`$(basename "$previous_report")\`"
    else
        echo "- Previous report: none"
    fi
    echo ""
    echo "## Commands"
    echo ""
    echo "- Runtime/E2E: \`${runtime_cmd_display}\`"
    echo "- Compile (<=${COMPILE_TIMEOUT_SEC}s): \`timeout ${COMPILE_TIMEOUT_SEC}s ${compile_cmd_display}\`"
    echo ""
    echo "## Status"
    echo ""
    echo "| Section | Status | Note |"
    echo "|---|---|---|"
    if [ "$runtime_status" -eq 0 ]; then
        echo "| runtime/e2e | pass | \`${RUNTIME_LOG_REL}\` |"
    else
        echo "| runtime/e2e | fail | exit=$runtime_status, see \`${RUNTIME_LOG_REL}\` |"
    fi
    if [ "$compile_status" -eq 0 ]; then
        echo "| compile | pass | wall=${compile_wall_ms}ms, log=\`${COMPILE_LOG_REL}\` |"
    elif [ "$compile_timed_out" -eq 1 ]; then
        echo "| compile | fail | timeout>${COMPILE_TIMEOUT_SEC}s, log=\`${COMPILE_LOG_REL}\` |"
    else
        echo "| compile | fail | exit=$compile_status, log=\`${COMPILE_LOG_REL}\` |"
    fi
    echo ""
    echo "## Full Compile Latency"
    echo ""
    echo "| Scenario | ns/iter | Delta vs prev |"
    echo "|---|---:|---:|"
    for scenario in simple multitask complex modal; do
        key="compile.full.${scenario}_ns_per_iter"
        cur="${metric_values[$key]}"
        prev="${prev_values[$key]:-}"
        echo "| $scenario | $(format_si "$cur") | $(metric_delta "$cur" "$prev") |"
    done
    echo ""
    echo "## Runtime Deadline Miss Rate"
    echo ""
    echo "| Clock | miss_rate_pct | Delta vs prev |"
    echo "|---|---:|---:|"
    for item in \
        "1kHz|runtime.thread.deadline_1khz_miss_rate_pct" \
        "10kHz|runtime.thread.deadline_10khz_miss_rate_pct" \
        "48kHz|runtime.thread.deadline_48khz_miss_rate_pct"; do
        label="${item%%|*}"
        key="${item##*|}"
        cur="${metric_values[$key]}"
        prev="${prev_values[$key]:-}"
        echo "| $label | $(format_si "$cur") | $(metric_delta "$cur" "$prev") |"
    done
    echo ""
    echo "## Ring Buffer Contention"
    echo ""
    echo "| Readers | reader_tokens_per_sec | Delta vs prev |"
    echo "|---:|---:|---:|"
    for item in \
        "1|runtime.ringbuf.contention_1reader_tokens_per_sec" \
        "2|runtime.ringbuf.contention_2readers_tokens_per_sec" \
        "4|runtime.ringbuf.contention_4readers_tokens_per_sec" \
        "8|runtime.ringbuf.contention_8readers_tokens_per_sec"; do
        readers="${item%%|*}"
        key="${item##*|}"
        cur="${metric_values[$key]}"
        prev="${prev_values[$key]:-}"
        echo "| $readers | $(format_si "$cur") | $(metric_delta "$cur" "$prev") |"
    done
    echo ""
    echo "## E2E Throughput"
    echo ""
    echo "| Benchmark | samples_per_sec | Delta vs prev |"
    echo "|---|---:|---:|"
    for item in \
        "Pipeline/64|e2e.pipeline_64_samples_per_sec" \
        "Pipeline/256|e2e.pipeline_256_samples_per_sec" \
        "Pipeline/1024|e2e.pipeline_1024_samples_per_sec" \
        "Socket/64 rx|e2e.socket_64_rx_samples_per_sec" \
        "Socket/256 rx|e2e.socket_256_rx_samples_per_sec" \
        "Socket/1024 rx|e2e.socket_1024_rx_samples_per_sec"; do
        label="${item%%|*}"
        key="${item##*|}"
        cur="${metric_values[$key]}"
        prev="${prev_values[$key]:-}"
        echo "| $label | $(format_si "$cur") | $(metric_delta "$cur" "$prev") |"
    done
    echo ""
    echo "- Socket benchmark errors: \`$socket_error_count\`"
    if [ "$socket_error_message" != "-" ]; then
        echo "- Socket error message: $socket_error_message"
    fi
    echo ""
    echo "## KPI Snapshot (Stable Keys)"
    echo ""
    echo "| Key | Value | Unit | Delta vs prev |"
    echo "|---|---:|---|---:|"
    for key in "${metric_keys[@]}"; do
        cur="${metric_values[$key]}"
        unit="${metric_units[$key]}"
        prev="${prev_values[$key]:-}"
        echo "| \`$key\` | $(format_si "$cur") | $unit | $(metric_delta "$cur" "$prev") |"
    done
    echo ""
    echo "## Artifacts"
    echo ""
    echo "- Runtime log: \`${RUNTIME_LOG_REL}\`"
    echo "- Compile log: \`${COMPILE_LOG_REL}\`"
    echo "- Runtime JSON dir: \`${RUNTIME_OUT_DIR_REL}\`"
    echo ""
    echo "## Machine Readable Metrics"
    echo ""
    echo "<!-- PIPIT_METRICS_BEGIN -->"
    for key in "${metric_keys[@]}"; do
        echo "$key|${metric_values[$key]}|${metric_units[$key]}"
    done
    echo "<!-- PIPIT_METRICS_END -->"
} >"$REPORT_TMP"

cp "$REPORT_TMP" "$REPORT_FILE"
git -C "$PROJECT_ROOT" add "$REPORT_FILE"
log "report generated and staged: $REPORT_FILE"

failed=false
if [ "$runtime_status" -ne 0 ]; then
    failed=true
    warn "runtime/e2e benchmark failed (exit=$runtime_status)"
fi
if [ "$compile_status" -ne 0 ]; then
    failed=true
    if [ "$compile_timed_out" -eq 1 ]; then
        warn "compile benchmark timed out (${COMPILE_TIMEOUT_SEC}s)"
    else
        warn "compile benchmark failed (exit=$compile_status)"
    fi
fi

if [ "$failed" = true ] && [ "$STRICT" = true ]; then
    exit 1
fi

exit 0
