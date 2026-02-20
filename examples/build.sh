#!/usr/bin/env bash
# Build all example PDL files into examples/build/
set -euo pipefail
cd "$(dirname "$0")/.."

echo "Building pcc..."
cargo build -p pcc --release

echo "Cleaning previous build..."
rm -rf examples/build/_cmake

echo "Configuring examples..."
cmake -S examples -B examples/build/_cmake -DCMAKE_BUILD_TYPE=Release "$@"

echo "Compiling examples..."
cmake --build examples/build/_cmake -j "$(nproc)"

echo "Done. Binaries in examples/build/"
