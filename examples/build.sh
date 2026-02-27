#!/usr/bin/env bash
# Build all example PDL files into examples/build/
#
# Usage:
#   ./examples/build.sh                # manifest workflow (default)
#   ./examples/build.sh --no-manifest  # direct header scanning
set -euo pipefail
cd "$(dirname "$0")/.."

PCC="$(pwd)/target/release/pcc"
MANIFEST_OPT="-DPIPIT_USE_MANIFEST=ON"
CMAKE_ARGS=()
for arg in "$@"; do
    case "$arg" in
        --no-manifest) MANIFEST_OPT="-DPIPIT_USE_MANIFEST=OFF" ;;
        *) CMAKE_ARGS+=("$arg") ;;
    esac
done

echo "Building pcc..."
cargo build -p pcc --release

echo "Cleaning previous build..."
rm -rf examples/build/_cmake

echo "Configuring examples..."
cmake -S examples -B examples/build/_cmake \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_COMPILER=clang++ \
    -DPCC="${PCC}" \
    ${MANIFEST_OPT} "${CMAKE_ARGS[@]}"

echo "Compiling examples..."
cmake --build examples/build/_cmake -j "$(nproc)"

echo "Done. Binaries in examples/build/"
