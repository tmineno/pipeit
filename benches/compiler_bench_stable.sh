#!/usr/bin/env bash
# Stable compiler benchmark runner for reproducible A/B measurements.
#
# Runs Criterion compiler KPIs with:
# - CPU pinning via taskset
# - fixed sample/warmup/measurement settings
# - optional baseline vs current comparison using git worktree

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MANIFEST_PATH="$PROJECT_ROOT/compiler/Cargo.toml"
BENCH_TARGET="compiler_bench"

CPU="${PIPIT_BENCH_CPU:-1}"
BENCH_FILTER="kpi/full_compile_latency"
SAMPLE_SIZE=40
MEASUREMENT_TIME=1.0
WARMUP_TIME=0.2
BASELINE_REF=""
TARGET_DIR="${PIPIT_BENCH_TARGET_DIR:-$PROJECT_ROOT/target/stable_bench}"
BASELINE_NAME="stable-baseline"

usage() {
    cat <<'USAGE'
Usage: compiler_bench_stable.sh [options]

Options:
  --cpu <id>                CPU core for taskset pinning (default: 1 or $PIPIT_BENCH_CPU)
  --filter <criterion_re>   Criterion benchmark filter (default: kpi/full_compile_latency)
  --sample-size <n>         Criterion sample size (default: 40)
  --measurement-time <sec>  Criterion measurement time in seconds (default: 1.0)
  --warm-up-time <sec>      Criterion warm-up time in seconds (default: 0.2)
  --baseline-ref <ref>      Run baseline first using git worktree at <ref>, then current tree
  --target-dir <path>       Shared cargo target dir for valid Criterion A/B compare
                            (default: $PROJECT_ROOT/target/stable_bench)
  --help                    Show this help

Examples:
  ./compiler_bench_stable.sh
  ./compiler_bench_stable.sh --filter 'kpi/full_compile_latency/complex'
  ./compiler_bench_stable.sh --baseline-ref HEAD
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --cpu)
            CPU="$2"
            shift 2
            ;;
        --filter)
            BENCH_FILTER="$2"
            shift 2
            ;;
        --sample-size)
            SAMPLE_SIZE="$2"
            shift 2
            ;;
        --measurement-time)
            MEASUREMENT_TIME="$2"
            shift 2
            ;;
        --warm-up-time)
            WARMUP_TIME="$2"
            shift 2
            ;;
        --baseline-ref)
            BASELINE_REF="$2"
            shift 2
            ;;
        --target-dir)
            TARGET_DIR="$2"
            shift 2
            ;;
        --help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "ERROR: required command not found: $1" >&2
        exit 1
    fi
}

require_cmd cargo
require_cmd taskset
require_cmd git

if [[ ! -f "$MANIFEST_PATH" ]]; then
    echo "ERROR: compiler manifest not found: $MANIFEST_PATH" >&2
    exit 1
fi

print_env_notes() {
    local governor_file="/sys/devices/system/cpu/cpu${CPU}/cpufreq/scaling_governor"
    if [[ -r "$governor_file" ]]; then
        local governor
        governor="$(cat "$governor_file")"
        if [[ "$governor" != "performance" ]]; then
            echo "WARN: cpu${CPU} governor is '$governor' (performance is recommended)." >&2
        else
            echo "INFO: cpu${CPU} governor is performance."
        fi
    else
        echo "INFO: scaling governor not readable for cpu${CPU}; skipping governor check."
    fi
}

run_bench() {
    local repo_root="$1"
    local label="$2"
    shift 2
    local extra_criterion_args=("$@")
    echo ""
    echo "=== $label ==="
    echo "repo: $repo_root"
    echo "cpu: $CPU"
    echo "filter: $BENCH_FILTER"
    echo "sample-size: $SAMPLE_SIZE"
    echo "measurement-time: $MEASUREMENT_TIME"
    echo "warm-up-time: $WARMUP_TIME"
    echo "cargo-target-dir: $TARGET_DIR"
    (
        cd "$repo_root"
        export CARGO_TARGET_DIR="$TARGET_DIR"
        taskset -c "$CPU" cargo bench \
            --manifest-path compiler/Cargo.toml \
            --bench "$BENCH_TARGET" \
            -- "$BENCH_FILTER" \
            --sample-size "$SAMPLE_SIZE" \
            --measurement-time "$MEASUREMENT_TIME" \
            --warm-up-time "$WARMUP_TIME" \
            "${extra_criterion_args[@]}"
    )
}

print_env_notes

if [[ -n "$BASELINE_REF" ]]; then
    tmp_worktree="$(mktemp -d /tmp/pipit-bench-baseline.XXXXXX)"
    cleanup() {
        git -C "$PROJECT_ROOT" worktree remove "$tmp_worktree" >/dev/null 2>&1 || true
        rm -rf "$tmp_worktree"
    }
    trap cleanup EXIT

    echo "Preparing baseline worktree at ref: $BASELINE_REF"
    git -C "$PROJECT_ROOT" worktree add "$tmp_worktree" "$BASELINE_REF" >/dev/null
    run_bench "$tmp_worktree" "baseline ($BASELINE_REF)" --save-baseline "$BASELINE_NAME"
    run_bench "$PROJECT_ROOT" "current tree vs $BASELINE_REF" --baseline "$BASELINE_NAME"
    exit 0
fi

run_bench "$PROJECT_ROOT" "current tree"
