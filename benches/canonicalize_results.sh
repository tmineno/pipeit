#!/bin/bash
# canonicalize_results.sh - Normalize benchmark outputs into canonical JSON.
#
# Canonical JSON shape:
# {
#   "context": { ... },
#   "benchmarks": [ ... ]
# }
#
# Supported kinds:
#   gbench   : Google Benchmark JSON -> canonical JSON (metadata enriched)
#   compiler : Criterion bencher text -> canonical JSON
#   timer    : timer_bench text -> canonical JSON
#   latency  : latency_bench text -> canonical JSON
#   pdl      : pdl_bench text -> canonical JSON
#   perf     : perf output directory -> canonical JSON

set -euo pipefail

KIND=""
INPUT=""
OUTPUT=""
SUITE=""
RESULTS_DIR=""

usage() {
    cat <<'EOF'
Usage:
  canonicalize_results.sh --kind <kind> --output <file> [--input <file>] [--suite <name>] [--results-dir <dir>]

Options:
  --kind <kind>         One of: gbench, compiler, timer, latency, pdl, perf
  --input <file>        Input file path (required for all except perf)
  --output <file>       Output canonical JSON file
  --suite <name>        Suite name in context (default: kind)
  --results-dir <dir>   Results directory (required for perf kind)
  --help                Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --kind)
            KIND="$2"
            shift 2
            ;;
        --input)
            INPUT="$2"
            shift 2
            ;;
        --output)
            OUTPUT="$2"
            shift 2
            ;;
        --suite)
            SUITE="$2"
            shift 2
            ;;
        --results-dir)
            RESULTS_DIR="$2"
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

if [ -z "$KIND" ]; then
    echo "--kind is required" >&2
    exit 1
fi
if [ -z "$OUTPUT" ]; then
    echo "--output is required" >&2
    exit 1
fi
if [ -z "$SUITE" ]; then
    SUITE="$KIND"
fi
if [ "$KIND" != "perf" ] && [ -z "$INPUT" ]; then
    echo "--input is required for kind '$KIND'" >&2
    exit 1
fi
if [ "$KIND" = "perf" ] && [ -z "$RESULTS_DIR" ]; then
    echo "--results-dir is required for kind 'perf'" >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "jq is required" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"

emit_empty() {
    jq -n \
      --arg date "$(date -Iseconds)" \
      --arg kind "$KIND" \
      --arg suite "$SUITE" \
      --arg input "$INPUT" \
      '{
         context: {
           date: $date,
           canonical_version: "v1",
           source_kind: $kind,
           suite: $suite,
           source_input: $input
         },
         benchmarks: []
       }' >"$OUTPUT"
}

canonicalize_gbench() {
    if [ ! -f "$INPUT" ]; then
        emit_empty
        return 0
    fi
    jq \
      --arg date "$(date -Iseconds)" \
      --arg suite "$SUITE" \
      '.context = ((.context // {}) + {
        canonical_version: "v1",
        canonicalized_at: $date,
        source_kind: "gbench",
        suite: $suite
      })' \
      "$INPUT" >"$OUTPUT"
}

canonicalize_compiler() {
    if [ ! -f "$INPUT" ]; then
        emit_empty
        return 0
    fi

    # Parse bencher-like lines:
    # test parse/simple ... bench:   123,456 ns/iter (+/- ...)
    local rows
    rows="$(
        awk '
        /bench:[[:space:]]*[0-9,]+[[:space:]]*(ns|us|ms|s)\/iter/ {
            name = ""
            if (match($0, /^test[[:space:]]+([^[:space:]]+)/, n)) {
                name = n[1]
            } else {
                next
            }
            value = ""
            unit = ""
            if (match($0, /bench:[[:space:]]*([0-9,]+)[[:space:]]*(ns|us|ms|s)\/iter/, m)) {
                value = m[1]
                unit = m[2]
            } else {
                next
            }
            gsub(/,/, "", value)
            if (unit == "us") value = value * 1000
            else if (unit == "ms") value = value * 1000000
            else if (unit == "s") value = value * 1000000000
            printf "%s\t%s\n", name, value
        }' "$INPUT"
    )"

    jq -n \
      --arg date "$(date -Iseconds)" \
      --arg input "$INPUT" \
      --arg suite "$SUITE" \
      --arg rows "$rows" '
      {
        context: {
          date: $date,
          canonical_version: "v1",
          source_kind: "compiler_bencher_text",
          suite: $suite,
          source_input: $input
        },
        benchmarks: (
          ($rows | split("\n") | map(select(length > 0) | split("\t")))
          | map({
              name: ("Compiler/" + .[0]),
              run_name: ("Compiler/" + .[0]),
              cpu_time: (.[1] | tonumber),
              real_time: (.[1] | tonumber),
              time_unit: "ns",
              iterations: 1
            })
        )
      }' >"$OUTPUT"
}

canonicalize_timer() {
    if [ ! -f "$INPUT" ]; then
        emit_empty
        return 0
    fi
    local rows
    rows="$(
        awk '
        /^\[timer_bench\]/ && /freq=/ && /avg=/ {
            line = $0
            sub(/^\[timer_bench\][[:space:]]*/, "", line)
            label = line
            sub(/[[:space:]]+freq=.*/, "", label)
            gsub(/[[:space:]]+$/, "", label)

            freq="0"; ticks="0"; overruns="0"; minv="0"; avgv="0"; medv="0"; p90="0"; p99="0"; p999="0"; maxv="0"
            if (match(line, /freq=([-0-9.]+)/, m)) freq=m[1]
            if (match(line, /ticks=([-0-9.]+)/, m)) ticks=m[1]
            if (match(line, /overruns=([-0-9.]+)/, m)) overruns=m[1]
            if (match(line, /min=([-0-9.]+)/, m)) minv=m[1]
            if (match(line, /avg=([-0-9.]+)/, m)) avgv=m[1]
            if (match(line, /median=([-0-9.]+)/, m)) medv=m[1]
            if (match(line, /p90=([-0-9.]+)/, m)) p90=m[1]
            if (match(line, /p99=([-0-9.]+)/, m)) p99=m[1]
            if (match(line, /p99\.9=([-0-9.]+)/, m)) p999=m[1]
            if (match(line, /max=([-0-9.]+)/, m)) maxv=m[1]
            printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", label, freq, ticks, overruns, minv, avgv, medv, p90, p99, p999, maxv
        }' "$INPUT"
    )"

    jq -n \
      --arg date "$(date -Iseconds)" \
      --arg input "$INPUT" \
      --arg suite "$SUITE" \
      --arg rows "$rows" '
      {
        context: {
          date: $date,
          canonical_version: "v1",
          source_kind: "timer_text",
          suite: $suite,
          source_input: $input
        },
        benchmarks: (
          ($rows | split("\n") | map(select(length > 0) | split("\t")))
          | map({
              name: ("Timer/" + .[0]),
              run_name: ("Timer/" + .[0]),
              cpu_time: (.[5] | tonumber),
              real_time: (.[5] | tonumber),
              time_unit: "ns",
              iterations: (.[2] | tonumber),
              freq_hz: (.[1] | tonumber),
              overruns: (.[3] | tonumber),
              min_ns: (.[4] | tonumber),
              avg_ns: (.[5] | tonumber),
              median_ns: (.[6] | tonumber),
              p90_ns: (.[7] | tonumber),
              p99_ns: (.[8] | tonumber),
              p999_ns: (.[9] | tonumber),
              max_ns: (.[10] | tonumber)
            })
        )
      }' >"$OUTPUT"
}

canonicalize_latency() {
    if [ ! -f "$INPUT" ]; then
        emit_empty
        return 0
    fi
    local rows
    rows="$(
        awk '
        /^\[latency\]/ && / n=/ && / avg=/ {
            line = $0
            sub(/^\[latency\][[:space:]]*/, "", line)
            label = line
            sub(/[[:space:]]+n=.*/, "", label)
            gsub(/[[:space:]]+$/, "", label)

            n="0"; minv="0"; avgv="0"; medv="0"; p90="0"; p99="0"; p999="0"; maxv="0"
            if (match(line, /n=([-0-9.]+)/, m)) n=m[1]
            if (match(line, /min=([-0-9.]+)/, m)) minv=m[1]
            if (match(line, /avg=([-0-9.]+)/, m)) avgv=m[1]
            if (match(line, /med=([-0-9.]+)/, m)) medv=m[1]
            if (match(line, /p90=([-0-9.]+)/, m)) p90=m[1]
            if (match(line, /p99=([-0-9.]+)/, m)) p99=m[1]
            if (match(line, /p999=([-0-9.]+)/, m)) p999=m[1]
            if (match(line, /max=([-0-9.]+)/, m)) maxv=m[1]
            printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", label, n, minv, avgv, medv, p90, p99, p999, maxv
        }' "$INPUT"
    )"

    jq -n \
      --arg date "$(date -Iseconds)" \
      --arg input "$INPUT" \
      --arg suite "$SUITE" \
      --arg rows "$rows" '
      {
        context: {
          date: $date,
          canonical_version: "v1",
          source_kind: "latency_text",
          suite: $suite,
          source_input: $input
        },
        benchmarks: (
          ($rows | split("\n") | map(select(length > 0) | split("\t")))
          | map({
              name: ("Latency/" + .[0]),
              run_name: ("Latency/" + .[0]),
              cpu_time: (.[3] | tonumber),
              real_time: (.[3] | tonumber),
              time_unit: "ns",
              iterations: (.[1] | tonumber),
              min_ns: (.[2] | tonumber),
              avg_ns: (.[3] | tonumber),
              median_ns: (.[4] | tonumber),
              p90_ns: (.[5] | tonumber),
              p99_ns: (.[6] | tonumber),
              p999_ns: (.[7] | tonumber),
              max_ns: (.[8] | tonumber)
            })
        )
      }' >"$OUTPUT"
}

canonicalize_pdl() {
    if [ ! -f "$INPUT" ]; then
        emit_empty
        return 0
    fi
    local rows
    rows="$(
        awk '
        /^Compiling[[:space:]]+[^[:space:]]+\.pdl\.\.\.$/ {
            prog = $2
            sub(/\.pdl\.\.\.$/, "", prog)
            next
        }
        /^\[stats\][[:space:]]+task / {
            if (prog == "") prog = "unknown"
            line = $0
            task = "unknown"
            ticks = 0
            missed = 0
            max_latency = 0
            avg_latency = 0
            if (match(line, /task '\''([^'\'']+)'\''/, m)) task = m[1]
            if (match(line, /ticks=([0-9]+)/, m)) ticks = m[1]
            if (match(line, /missed=([0-9]+)/, m)) missed = m[1]
            if (match(line, /max_latency=([0-9]+)ns/, m)) max_latency = m[1]
            if (match(line, /avg_latency=([0-9]+)ns/, m)) avg_latency = m[1]
            printf "task\t%s\t%s\t%s\t%s\t%s\t%s\n", prog, task, ticks, missed, max_latency, avg_latency
            next
        }
        /^\[stats\][[:space:]]+shared buffer / {
            if (prog == "") prog = "unknown"
            line = $0
            buf = "unknown"
            tokens = 0
            bytes = 0
            if (match(line, /shared buffer '\''([^'\'']+)'\''/, m)) buf = m[1]
            if (match(line, /: ([0-9]+) tokens/, m)) tokens = m[1]
            if (match(line, /\(([0-9]+)B\)/, m)) bytes = m[1]
            printf "buffer\t%s\t%s\t%s\t%s\n", prog, buf, tokens, bytes
            next
        }' "$INPUT"
    )"

    jq -n \
      --arg date "$(date -Iseconds)" \
      --arg input "$INPUT" \
      --arg suite "$SUITE" \
      --arg rows "$rows" '
      def parse_row($row):
        ($row | split("\t")) as $f
        | if $f[0] == "task" then
            {
              name: ("pdl/" + $f[1] + "/task:" + $f[2]),
              run_name: ("pdl/" + $f[1] + "/task:" + $f[2]),
              cpu_time: ($f[6] | tonumber),
              real_time: ($f[6] | tonumber),
              time_unit: "ns",
              iterations: ($f[3] | tonumber),
              suite: "pdl",
              scenario: $f[1],
              variant: ("task:" + $f[2]),
              missed: ($f[4] | tonumber),
              max_latency_ns: ($f[5] | tonumber),
              avg_latency_ns: ($f[6] | tonumber)
            }
          elif $f[0] == "buffer" then
            {
              name: ("pdl/" + $f[1] + "/buffer:" + $f[2]),
              run_name: ("pdl/" + $f[1] + "/buffer:" + $f[2]),
              cpu_time: ($f[3] | tonumber),
              real_time: ($f[3] | tonumber),
              time_unit: "count",
              iterations: 1,
              suite: "pdl",
              scenario: $f[1],
              variant: ("buffer:" + $f[2]),
              tokens: ($f[3] | tonumber),
              bytes: ($f[4] | tonumber)
            }
          else empty end;
      {
        context: {
          date: $date,
          canonical_version: "v1",
          source_kind: "pdl_text",
          suite: $suite,
          source_input: $input
        },
        benchmarks: (
          $rows
          | split("\n")
          | map(select(length > 0) | parse_row(.))
        )
      }' >"$OUTPUT"
}

canonicalize_perf() {
    if [ ! -d "$RESULTS_DIR" ]; then
        emit_empty
        return 0
    fi

    local rows
    rows="$(
        find "$RESULTS_DIR" -maxdepth 1 -type f \( -name 'perf_*.txt' -o -name 'perf_*.json' \) | sort |
            while read -r f; do
                base="$(basename "$f")"
                ext="${base##*.}"
                seconds="0"
                if [ "$ext" = "txt" ]; then
                    seconds="$(
                        awk '
                        /seconds time elapsed/ {
                            gsub(/,/, "", $1)
                            print $1
                            exit
                        }' "$f"
                    )"
                elif [ "$ext" = "json" ]; then
                    seconds="$(
                        jq -r '
                        def to_num:
                          if type == "number" then .
                          elif type == "string" then (gsub(","; "") | tonumber?)
                          else null end;
                        if type == "array" then
                          ([ .[]
                             | select(((.event? // "") == "seconds time elapsed") or ((.event? // "") == "duration_time"))
                             | (.["counter-value"] // .counter_value // .value)
                             | to_num
                           ] | add // 0)
                        elif type == "object" then
                          ([ .[]?
                             | select(((.event? // "") == "seconds time elapsed") or ((.event? // "") == "duration_time"))
                             | (.["counter-value"] // .counter_value // .value)
                             | to_num
                           ] | add // 0)
                        else 0 end
                        ' "$f" 2>/dev/null || echo "0"
                    )"
                fi
                if [ -z "$seconds" ]; then
                    seconds="0"
                fi
                if ! [[ "$seconds" =~ ^-?[0-9]+([.][0-9]+)?$ ]]; then
                    seconds="0"
                fi
                lines="$(wc -l <"$f" | tr -d ' ')"
                bytes="$(wc -c <"$f" | tr -d ' ')"
                printf "%s\t%s\t%s\t%s\t%s\n" "$base" "$ext" "$seconds" "$lines" "$bytes"
            done
    )"

    jq -n \
      --arg date "$(date -Iseconds)" \
      --arg results_dir "$RESULTS_DIR" \
      --arg suite "$SUITE" \
      --arg rows "$rows" '
      def parse_name($fname):
        ($fname | sub("^perf_"; "") | sub("\\.(txt|json)$"; "")) as $stem
        | ($stem | split("_")) as $parts
        | {
            scenario: ($parts[0] // "unknown"),
            variant: (if ($parts | length) > 1 then ($parts[1:] | join("_")) else "default" end)
          };
      {
        context: {
          date: $date,
          canonical_version: "v1",
          source_kind: "perf_text_dir",
          suite: $suite,
          source_input: $results_dir
        },
        benchmarks: (
          ($rows | split("\n") | map(select(length > 0) | split("\t")))
          | map(. as $r | (parse_name($r[0])) as $p | {
              name: ("perf/" + $p.scenario + "/" + $p.variant),
              run_name: ("perf/" + $p.scenario + "/" + $p.variant),
              cpu_time: ($r[2] | tonumber),
              real_time: ($r[2] | tonumber),
              time_unit: "s",
              iterations: 1,
              suite: "perf",
              scenario: $p.scenario,
              variant: $p.variant,
              source_file: $r[0],
              source_format: $r[1],
              lines: ($r[3] | tonumber),
              bytes: ($r[4] | tonumber)
            })
        )
      }' >"$OUTPUT"
}

case "$KIND" in
    gbench) canonicalize_gbench ;;
    compiler) canonicalize_compiler ;;
    timer) canonicalize_timer ;;
    latency) canonicalize_latency ;;
    pdl) canonicalize_pdl ;;
    perf) canonicalize_perf ;;
    *)
        echo "Unsupported --kind: $KIND" >&2
        exit 1
        ;;
esac
