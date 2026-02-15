#!/bin/bash
# perf_common.sh — Shared utilities for perf-based benchmarks
#
# Sourced by individual perf scripts. Not run directly.
# Provides environment probing, build helpers, and perf wrappers.

set -e

# ── Standard paths ────────────────────────────────────────────────────────

PERF_SCRIPT_DIR="${PERF_SCRIPT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
BENCH_DIR="${BENCH_DIR:-$(dirname "$PERF_SCRIPT_DIR")}"
PROJECT_ROOT="${PROJECT_ROOT:-$(dirname "$BENCH_DIR")}"
RUNTIME_INCLUDE="${RUNTIME_INCLUDE:-$PROJECT_ROOT/runtime/libpipit/include}"
EXAMPLES_DIR="${EXAMPLES_DIR:-$PROJECT_ROOT/examples}"
BUILD_DIR="${BUILD_DIR:-/tmp/pipit_perf_build_$$}"
OUTPUT_DIR="${OUTPUT_DIR:-$BENCH_DIR/results}"
CXX="${CXX:-c++}"
CXX_FLAGS="${CXX_FLAGS:--std=c++20 -O3 -march=native -DNDEBUG}"

# Detect Google Benchmark library path
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

# ── Environment probing: perf events ─────────────────────────────────────

# Probe whether a single perf event actually works on this system.
# Returns 0 if event works, 1 if not (some events fail silently on WSL2).
probe_event() {
    local event="$1"
    local output
    output=$(perf stat -e "$event" -- true 2>&1)
    # Check for "<not supported>" or "<not counted>" in output
    # Use || true to prevent set -e from killing us when grep finds no match
    if echo "$output" | grep -qE '<not supported>|<not counted>' 2>/dev/null; then
        return 1
    fi
    return 0
}

# Probe a standard set of events and export AVAIL_EVENTS (comma-separated).
# Also exports per-category variables for convenience.
probe_available_events() {
    local all_events=(
        cpu-cycles instructions
        branch-instructions branch-misses
        cache-references cache-misses
        L1-dcache-loads L1-dcache-load-misses L1-dcache-prefetches
        L1-icache-loads L1-icache-load-misses
        dTLB-loads dTLB-load-misses
        iTLB-loads iTLB-load-misses
        stalled-cycles-frontend
        page-faults minor-faults major-faults
        context-switches cpu-migrations
    )

    AVAIL_EVENTS=""
    AVAIL_HW_EVENTS=""
    AVAIL_CACHE_EVENTS=""
    AVAIL_BRANCH_EVENTS=""
    AVAIL_TLB_EVENTS=""
    AVAIL_SW_EVENTS=""

    for ev in "${all_events[@]}"; do
        if probe_event "$ev"; then
            [ -n "$AVAIL_EVENTS" ] && AVAIL_EVENTS="${AVAIL_EVENTS},"
            AVAIL_EVENTS="${AVAIL_EVENTS}${ev}"

            case "$ev" in
                cpu-cycles|instructions|stalled-cycles-frontend)
                    [ -n "$AVAIL_HW_EVENTS" ] && AVAIL_HW_EVENTS="${AVAIL_HW_EVENTS},"
                    AVAIL_HW_EVENTS="${AVAIL_HW_EVENTS}${ev}"
                    ;;
                cache-*|L1-*)
                    [ -n "$AVAIL_CACHE_EVENTS" ] && AVAIL_CACHE_EVENTS="${AVAIL_CACHE_EVENTS},"
                    AVAIL_CACHE_EVENTS="${AVAIL_CACHE_EVENTS}${ev}"
                    ;;
                branch-*)
                    [ -n "$AVAIL_BRANCH_EVENTS" ] && AVAIL_BRANCH_EVENTS="${AVAIL_BRANCH_EVENTS},"
                    AVAIL_BRANCH_EVENTS="${AVAIL_BRANCH_EVENTS}${ev}"
                    ;;
                *TLB*)
                    [ -n "$AVAIL_TLB_EVENTS" ] && AVAIL_TLB_EVENTS="${AVAIL_TLB_EVENTS},"
                    AVAIL_TLB_EVENTS="${AVAIL_TLB_EVENTS}${ev}"
                    ;;
                page-faults|minor-faults|major-faults|context-switches|cpu-migrations)
                    [ -n "$AVAIL_SW_EVENTS" ] && AVAIL_SW_EVENTS="${AVAIL_SW_EVENTS},"
                    AVAIL_SW_EVENTS="${AVAIL_SW_EVENTS}${ev}"
                    ;;
            esac
        fi
    done

    export AVAIL_EVENTS AVAIL_HW_EVENTS AVAIL_CACHE_EVENTS
    export AVAIL_BRANCH_EVENTS AVAIL_TLB_EVENTS AVAIL_SW_EVENTS
}

# ── Environment probing: CPU topology ────────────────────────────────────

# Parse lscpu -e to discover CPU topology. Sets:
#   CPU_SMT_A, CPU_SMT_B       — two CPUs on the same physical core
#   CPU_NEAR_A, CPU_NEAR_B     — two CPUs on adjacent physical cores
#   CPU_FAR_A, CPU_FAR_B       — two CPUs on most-distant physical cores
#   PHYSICAL_CORE_CPUS         — array: one representative CPU per physical core
#   NUM_PHYSICAL_CORES         — count of physical cores
#   NUM_LOGICAL_CPUS           — count of logical CPUs
probe_topology() {
    local -A core_to_cpus  # core_id -> space-separated CPU list
    local -a core_order    # ordered list of unique core IDs

    # Parse lscpu output (skip header)
    while IFS= read -r line; do
        # Skip header
        [[ "$line" =~ ^CPU ]] && continue
        # Parse fields: CPU CORE SOCKET NODE
        local cpu core socket node
        read -r cpu core socket node <<< "$line"
        [ -z "$cpu" ] && continue

        if [ -z "${core_to_cpus[$core]+x}" ]; then
            core_order+=("$core")
        fi
        core_to_cpus[$core]="${core_to_cpus[$core]:-} $cpu"
    done < <(lscpu -e=CPU,CORE,SOCKET,NODE 2>/dev/null)

    NUM_LOGICAL_CPUS=$(nproc 2>/dev/null || echo 1)
    NUM_PHYSICAL_CORES=${#core_order[@]}

    # Build PHYSICAL_CORE_CPUS: first CPU from each physical core
    PHYSICAL_CORE_CPUS=()
    for cid in "${core_order[@]}"; do
        local cpus
        read -ra cpus <<< "${core_to_cpus[$cid]}"
        PHYSICAL_CORE_CPUS+=("${cpus[0]}")
    done

    # Pick SMT pair: first core that has 2+ CPUs
    CPU_SMT_A=""
    CPU_SMT_B=""
    for cid in "${core_order[@]}"; do
        local cpus
        read -ra cpus <<< "${core_to_cpus[$cid]}"
        if [ ${#cpus[@]} -ge 2 ]; then
            CPU_SMT_A="${cpus[0]}"
            CPU_SMT_B="${cpus[1]}"
            break
        fi
    done
    # Fallback if no SMT
    if [ -z "$CPU_SMT_A" ]; then
        CPU_SMT_A="${PHYSICAL_CORE_CPUS[0]:-0}"
        CPU_SMT_B="${PHYSICAL_CORE_CPUS[0]:-0}"
    fi

    # Pick adjacent cores: first two distinct physical cores
    CPU_NEAR_A="${PHYSICAL_CORE_CPUS[0]:-0}"
    CPU_NEAR_B="${PHYSICAL_CORE_CPUS[1]:-0}"

    # Pick distant cores: first and last physical cores (maximizes distance)
    CPU_FAR_A="${PHYSICAL_CORE_CPUS[0]:-0}"
    local last_idx=$((NUM_PHYSICAL_CORES - 1))
    CPU_FAR_B="${PHYSICAL_CORE_CPUS[$last_idx]:-0}"

    export CPU_SMT_A CPU_SMT_B CPU_NEAR_A CPU_NEAR_B CPU_FAR_A CPU_FAR_B
    export PHYSICAL_CORE_CPUS NUM_PHYSICAL_CORES NUM_LOGICAL_CPUS
}

# ── Environment probing: cache sizes ─────────────────────────────────────

# Read cache sizes from sysfs. Sets:
#   CACHE_L1D_KB, CACHE_L1I_KB, CACHE_L2_KB, CACHE_L3_KB
probe_cache_sizes() {
    CACHE_L1D_KB=0
    CACHE_L1I_KB=0
    CACHE_L2_KB=0
    CACHE_L3_KB=0

    local cache_base="/sys/devices/system/cpu/cpu0/cache"
    if [ ! -d "$cache_base" ]; then
        # Fallback: try to parse from lscpu
        CACHE_L1D_KB=$(lscpu 2>/dev/null | grep 'L1d cache' | grep -oP '\d+' | head -1)
        CACHE_L1I_KB=$(lscpu 2>/dev/null | grep 'L1i cache' | grep -oP '\d+' | head -1)
        CACHE_L2_KB=$(lscpu 2>/dev/null | grep 'L2 cache' | grep -oP '\d+' | head -1)
        CACHE_L3_KB=$(lscpu 2>/dev/null | grep 'L3 cache' | grep -oP '\d+' | head -1)
        # lscpu may report in MiB; convert
        return
    fi

    for idx_dir in "$cache_base"/index*; do
        [ -d "$idx_dir" ] || continue
        local level type size_str
        level=$(cat "$idx_dir/level" 2>/dev/null || echo 0)
        type=$(cat "$idx_dir/type" 2>/dev/null || echo "Unknown")
        size_str=$(cat "$idx_dir/size" 2>/dev/null || echo "0K")

        # Parse size: "32K", "512K", "16384K", "32M", etc.
        local size_kb=0
        if [[ "$size_str" =~ ^([0-9]+)K$ ]]; then
            size_kb="${BASH_REMATCH[1]}"
        elif [[ "$size_str" =~ ^([0-9]+)M$ ]]; then
            size_kb=$(( BASH_REMATCH[1] * 1024 ))
        fi

        case "$level" in
            1)
                case "$type" in
                    Data) CACHE_L1D_KB=$size_kb ;;
                    Instruction) CACHE_L1I_KB=$size_kb ;;
                esac
                ;;
            2) CACHE_L2_KB=$size_kb ;;
            3) CACHE_L3_KB=$size_kb ;;
        esac
    done

    export CACHE_L1D_KB CACHE_L1I_KB CACHE_L2_KB CACHE_L3_KB
}

# ── Environment probing: NUMA ────────────────────────────────────────────

# Check NUMA topology. Sets:
#   NUMA_TOPOLOGY — "single" or "multi"
#   NUMA_NODES    — number of NUMA nodes
probe_numa() {
    NUMA_NODES=1
    NUMA_TOPOLOGY="single"

    if command -v numactl &>/dev/null; then
        local nodes
        nodes=$(numactl --hardware 2>/dev/null | grep "available:" | awk '{print $2}')
        if [ -n "$nodes" ] && [ "$nodes" -gt 1 ] 2>/dev/null; then
            NUMA_NODES=$nodes
            NUMA_TOPOLOGY="multi"
        fi
    fi

    export NUMA_TOPOLOGY NUMA_NODES
}

# ── Build helper ─────────────────────────────────────────────────────────

# Build a C++ benchmark binary. Returns path to executable via stdout.
# Usage: exe=$(build_bench source.cpp name [extra_flags])
build_bench() {
    local src="$1" name="$2" extra="${3:-}"
    local exe="$BUILD_DIR/$name"

    $CXX $CXX_FLAGS -I "$RUNTIME_INCLUDE" -I "$EXAMPLES_DIR" \
         $extra "$src" $BENCH_LIB_FLAGS -o "$exe" 2>/dev/null
    echo "$exe"
}

# Build without Google Benchmark (for custom measurement programs)
build_bench_custom() {
    local src="$1" name="$2" extra="${3:-}"
    local exe="$BUILD_DIR/$name"

    $CXX $CXX_FLAGS -I "$RUNTIME_INCLUDE" -I "$EXAMPLES_DIR" \
         $extra "$src" -lpthread -o "$exe" 2>/dev/null
    echo "$exe"
}

# ── Perf wrappers ────────────────────────────────────────────────────────

# Run perf stat with text output (human-readable).
# Usage: perf_stat_text events executable [args] [repeats]
perf_stat_text() {
    local events="$1" exe="$2" args="${3:-}" repeats="${4:-5}"

    if [ -z "$events" ]; then
        echo "  WARNING: No events available for this measurement. Skipping."
        return 1
    fi

    local safe_name
    safe_name="$(basename "$exe")_$(echo "$events" | tr ',/' '__' | cut -c1-80)"
    local outfile="$OUTPUT_DIR/perf_${safe_name}.txt"

    perf stat -r "$repeats" -e "$events" -- $exe $args 2>"$outfile"
    # Also print summary to stdout
    cat "$outfile"
    echo "  -> $outfile"
}

# Run perf stat with JSON output.
# Usage: perf_stat_json events executable [args] [repeats]
perf_stat_json() {
    local events="$1" exe="$2" args="${3:-}" repeats="${4:-5}"

    if [ -z "$events" ]; then
        echo "  WARNING: No events available for this measurement. Skipping."
        return 1
    fi

    local safe_name
    safe_name="$(basename "$exe")_$(echo "$events" | tr ',/' '__' | cut -c1-80)"
    local outfile="$OUTPUT_DIR/perf_${safe_name}.json"

    perf stat -j -r "$repeats" -e "$events" -- $exe $args 2>"$outfile"
    echo "$outfile"
}

# ── Output helpers ────────────────────────────────────────────────────────

# Print a standard header with probed hardware info
print_header() {
    local name="$1"
    echo -e "${BLUE}=== Pipit Perf: $name ===${NC}"
    echo "Date:     $(date -Iseconds)"
    echo "CPU:      $(lscpu 2>/dev/null | grep 'Model name' | sed 's/.*: *//' || echo 'unknown')"
    echo "Cores:    $NUM_PHYSICAL_CORES physical, $NUM_LOGICAL_CPUS logical"
    echo "Cache:    L1d=${CACHE_L1D_KB}KB L1i=${CACHE_L1I_KB}KB L2=${CACHE_L2_KB}KB L3=${CACHE_L3_KB}KB"
    echo "NUMA:     $NUMA_TOPOLOGY ($NUMA_NODES node(s))"
    echo "Topology: smt=($CPU_SMT_A,$CPU_SMT_B) near=($CPU_NEAR_A,$CPU_NEAR_B) far=($CPU_FAR_A,$CPU_FAR_B)"
    echo "Kernel:   $(uname -r)"
    echo ""
}

# Annotate which cache level a buffer size (in bytes) fits in
cache_level_for_size() {
    local size_bytes="$1"
    local size_kb=$(( size_bytes / 1024 ))

    if [ "$CACHE_L1D_KB" -gt 0 ] && [ "$size_kb" -le "$CACHE_L1D_KB" ]; then
        echo "L1d"
    elif [ "$CACHE_L2_KB" -gt 0 ] && [ "$size_kb" -le "$CACHE_L2_KB" ]; then
        echo "L2"
    elif [ "$CACHE_L3_KB" -gt 0 ] && [ "$size_kb" -le "$CACHE_L3_KB" ]; then
        echo "L3"
    else
        echo "DRAM"
    fi
}

# ── Initialization ───────────────────────────────────────────────────────

# Run all probes when sourced (unless PERF_SKIP_PROBE is set)
if [ -z "${PERF_SKIP_PROBE:-}" ]; then
    probe_topology
    probe_cache_sizes
    probe_numa
    probe_available_events
fi
