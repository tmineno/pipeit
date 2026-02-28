#!/usr/bin/env bash
# Build all example PDL files into examples/build/
#
# Usage:
#   ./examples/build.sh
set -euo pipefail
cd "$(dirname "$0")/.."

PCC="$(pwd)/target/release/pcc"
CMAKE_ARGS=()
for arg in "$@"; do
    CMAKE_ARGS+=("$arg")
done

echo "Building pcc..."
cargo build -p pcc --release

echo "Cleaning previous build..."
rm -rf examples/build/_cmake

echo "Configuring examples..."
cmake -S examples -B examples/build/_cmake \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_COMPILER=clang++ \
    -DPCC="${PCC}" "${CMAKE_ARGS[@]}"

echo "Compiling examples..."
cmake --build examples/build/_cmake -j "$(nproc)"

echo "Done. Binaries in examples/build/"
