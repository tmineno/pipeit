#!/bin/bash
# PDL Runtime Benchmarks
# Compiles and benchmarks generated PDL programs to measure end-to-end performance

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PDL_DIR="$SCRIPT_DIR/pdl"
BUILD_DIR="/tmp/pipit_bench_$$"
RUNTIME_INCLUDE="$SCRIPT_DIR/../runtime/libpipit/include"
ACTORS_HEADER="$SCRIPT_DIR/../examples/actors.h"
PCC="$SCRIPT_DIR/../target/release/pcc"

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== PDL Runtime Benchmarks ===${NC}"
echo -e "Build directory: ${BUILD_DIR}"
echo

# Build pcc if needed
if [ ! -f "$PCC" ]; then
    echo -e "${YELLOW}Building pcc compiler...${NC}"
    cd "$SCRIPT_DIR/.." && cargo build --release -p pcc
fi

# Create build directory
mkdir -p "$BUILD_DIR"

# Cleanup on exit
cleanup() {
    rm -rf "$BUILD_DIR"
}
trap cleanup EXIT

# Compile each PDL to executable
cd "$PDL_DIR"
for pdl in *.pdl; do
    name="${pdl%.pdl}"
    cpp_file="$BUILD_DIR/${name}_generated.cpp"
    exe="$BUILD_DIR/${name}_bench"

    echo -e "${GREEN}Compiling $pdl...${NC}"

    # Generate C++ with optimizations
    "$PCC" "$pdl" -I "$ACTORS_HEADER" --emit cpp -o "$cpp_file" 2>/dev/null || {
        echo "  ⚠ Skipping $pdl (compilation failed)"
        continue
    }

    # Compile to executable
    c++ -std=c++20 -O3 -march=native -DNDEBUG \
        -I "$RUNTIME_INCLUDE" -I "$(dirname "$ACTORS_HEADER")" \
        "$cpp_file" -lpthread -o "$exe" 2>/dev/null || {
        echo "  ⚠ Skipping $pdl (C++ compilation failed)"
        continue
    }

    echo "  ✓ Built ${exe##*/}"
done

echo
echo -e "${BLUE}=== Running Benchmarks ===${NC}"
echo

# Benchmark each executable
cd "$BUILD_DIR"
for exe in *_bench; do
    [ -x "$exe" ] || continue

    name="${exe%_bench}"
    echo -e "${GREEN}Benchmark: $name${NC}"

    # Run for 1 second and measure performance
    # The --stats flag will give us tick counts and latency
    ./"$exe" --duration 1s --stats 2>&1 | grep -E "^\[stats\]|ticks=|avg_latency=" || true

    echo
done

echo -e "${BLUE}=== Summary ===${NC}"
echo "Benchmarked $(ls -1 "$BUILD_DIR"/*_bench 2>/dev/null | wc -l) PDL programs"
echo "Generated C++ files: $(ls -1 "$BUILD_DIR"/*_generated.cpp 2>/dev/null | wc -l)"
echo
echo "For detailed profiling, re-run with custom duration:"
echo "  ./pdl_bench.sh"
echo "  # Or compile manually:"
echo "  cd $PDL_DIR"
echo "  $PCC simple.pdl -I $ACTORS_HEADER -o /tmp/simple && /tmp/simple --duration 10s --stats"
