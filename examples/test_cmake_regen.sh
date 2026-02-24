#!/usr/bin/env bash
# Smoke test: verify that touching an actor header triggers manifest + C++ regeneration.
#
# Prerequisites: pcc must be built (cargo build -p pcc --release)
set -euo pipefail
cd "$(dirname "$0")/.."

PCC="$(pwd)/target/release/pcc"
BUILD_DIR="examples/build/_cmake_test"

if [ ! -x "${PCC}" ]; then
    echo "SKIP: pcc not found at ${PCC}. Build it first: cargo build -p pcc --release"
    exit 0
fi

# Clean build
rm -rf "${BUILD_DIR}"
cmake -S examples -B "${BUILD_DIR}" -DCMAKE_BUILD_TYPE=Release -DPCC="${PCC}" > /dev/null 2>&1
cmake --build "${BUILD_DIR}" -j "$(nproc)" > /dev/null 2>&1

# Record timestamps
MANIFEST="${BUILD_DIR}/actors.meta.json"
GAIN_CPP="${BUILD_DIR}/gain.cpp"

if [ ! -f "${MANIFEST}" ]; then
    echo "FAIL: actors.meta.json not generated"
    rm -rf "${BUILD_DIR}"
    exit 1
fi
if [ ! -f "${GAIN_CPP}" ]; then
    echo "FAIL: gain.cpp not generated"
    rm -rf "${BUILD_DIR}"
    exit 1
fi

TS_MANIFEST_BEFORE=$(stat -c %Y "${MANIFEST}")
TS_CPP_BEFORE=$(stat -c %Y "${GAIN_CPP}")

# Touch a header, rebuild
sleep 1  # ensure timestamp changes
touch examples/example_actors.h
cmake --build "${BUILD_DIR}" -j "$(nproc)" > /dev/null 2>&1

TS_MANIFEST_AFTER=$(stat -c %Y "${MANIFEST}")
TS_CPP_AFTER=$(stat -c %Y "${GAIN_CPP}")

# Verify regeneration
FAILED=0
if [ "${TS_MANIFEST_AFTER}" -le "${TS_MANIFEST_BEFORE}" ]; then
    echo "FAIL: manifest was not regenerated after header touch"
    FAILED=1
fi
if [ "${TS_CPP_AFTER}" -le "${TS_CPP_BEFORE}" ]; then
    echo "FAIL: gain.cpp was not regenerated after manifest change"
    FAILED=1
fi

rm -rf "${BUILD_DIR}"

if [ "${FAILED}" -eq 1 ]; then
    exit 1
fi

echo "PASS: header touch → manifest regen → C++ regen"
