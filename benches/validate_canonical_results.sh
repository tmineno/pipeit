#!/bin/bash
# validate_canonical_results.sh - Validate canonical benchmark JSON artifacts.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INPUT_DIR="$SCRIPT_DIR/results"
SCHEMA_FILE="$SCRIPT_DIR/schema/canonical-benchmark.schema.json"
FILES=()

usage() {
    cat <<'EOF'
Usage: validate_canonical_results.sh [--input-dir <dir>] [--file <path>] [--schema <path>]

Options:
  --input-dir <dir>   Directory to scan for *.canonical.json (default: benches/results)
  --file <path>       Validate a specific canonical JSON file (repeatable)
  --schema <path>     Schema reference path for reporting (default: benches/schema/canonical-benchmark.schema.json)
  --help              Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --input-dir)
            INPUT_DIR="$2"
            shift 2
            ;;
        --file)
            FILES+=("$2")
            shift 2
            ;;
        --schema)
            SCHEMA_FILE="$2"
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

if ! command -v jq >/dev/null 2>&1; then
    echo "jq is required for validation" >&2
    exit 1
fi

if [ ${#FILES[@]} -eq 0 ]; then
    if [ ! -d "$INPUT_DIR" ]; then
        echo "Input directory not found: $INPUT_DIR" >&2
        exit 1
    fi
    while IFS= read -r -d '' f; do
        FILES+=("$f")
    done < <(find "$INPUT_DIR" -maxdepth 1 -type f -name '*.canonical.json' -print0 | sort -z)
fi

if [ ${#FILES[@]} -eq 0 ]; then
    echo "No canonical JSON files found to validate" >&2
    exit 1
fi

if [ ! -f "$SCHEMA_FILE" ]; then
    echo "Schema file not found: $SCHEMA_FILE" >&2
    exit 1
fi

echo "Validating canonical benchmark JSON files"
echo "Schema: $SCHEMA_FILE"
echo ""

PASS=0
FAIL=0

validate_core_shape() {
    local file="$1"
    jq -e '
      type == "object"
      and (.context | type == "object")
      and (.benchmarks | type == "array")
      and (.context.canonical_version | type == "string")
      and (.context.source_kind | type == "string")
      and (.context.suite | type == "string")
      and (
        [
          .benchmarks[]? |
          (
            type == "object"
            and (.name | type == "string")
            and (.run_name | type == "string")
            and (.cpu_time | type == "number")
            and (.real_time | type == "number")
            and (.time_unit | type == "string")
            and (.iterations | type == "number")
          )
        ] | all
      )
    ' "$file" >/dev/null
}

validate_pdl_naming() {
    local file="$1"
    jq -e '
      (.context.suite != "pdl")
      or (
        [
          .benchmarks[]? |
          (
            (.name | test("^pdl/[^/]+/(task:[^/]+|buffer:[^/]+)$"))
            and (.suite == "pdl")
            and (.scenario | type == "string")
            and (.variant | type == "string")
          )
        ] | all
      )
    ' "$file" >/dev/null
}

validate_perf_naming() {
    local file="$1"
    jq -e '
      (.context.suite != "perf")
      or (
        [
          .benchmarks[]? |
          (
            (.name | test("^perf/[^/]+/[^/]+$"))
            and (.suite == "perf")
            and (.scenario | type == "string")
            and (.variant | type == "string")
          )
        ] | all
      )
    ' "$file" >/dev/null
}

for file in "${FILES[@]}"; do
    if [ ! -f "$file" ]; then
        echo "FAIL: $file (file not found)"
        FAIL=$((FAIL + 1))
        continue
    fi

    if ! jq -e '.' "$file" >/dev/null 2>&1; then
        echo "FAIL: $file (invalid JSON)"
        FAIL=$((FAIL + 1))
        continue
    fi

    if ! validate_core_shape "$file"; then
        echo "FAIL: $file (core schema check failed)"
        FAIL=$((FAIL + 1))
        continue
    fi

    if ! validate_pdl_naming "$file"; then
        echo "FAIL: $file (pdl naming/schema check failed)"
        FAIL=$((FAIL + 1))
        continue
    fi

    if ! validate_perf_naming "$file"; then
        echo "FAIL: $file (perf naming/schema check failed)"
        FAIL=$((FAIL + 1))
        continue
    fi

    echo "PASS: $file"
    PASS=$((PASS + 1))
done

echo ""
echo "Validation summary: PASS=$PASS FAIL=$FAIL"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
