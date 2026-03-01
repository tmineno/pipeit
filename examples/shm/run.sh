#!/usr/bin/env bash
# run.sh — Build and run the SHM writer/reader example, then verify data exchange.
#
# Usage:
#   ./examples/shm/run.sh
#
# The script:
#   1. Builds pcc and the SHM examples via CMake
#   2. Starts the writer in the background
#   3. Waits briefly for the shm segment to be created
#   4. Runs the reader, capturing output
#   5. Verifies non-zero samples were received (data exchange succeeded)
set -euo pipefail
cd "$(dirname "$0")/../.."

WRITER_DURATION="${WRITER_DURATION:-5s}"
READER_DURATION="${READER_DURATION:-2s}"

echo "=== SHM Example: Writer → Reader via shared memory ==="
echo ""

# ── Step 1: Build ─────────────────────────────────────────────────────
echo "[1/5] Building pcc..."
cargo build -p pcc --release 2>&1 | tail -1

PCC="$(pwd)/target/release/pcc"

echo "[2/5] Building SHM examples..."
rm -rf examples/build/_cmake_shm
cmake -S examples -B examples/build/_cmake_shm \
    -DCMAKE_BUILD_TYPE=Release \
    -DPCC="${PCC}" \
    -DCMAKE_CXX_COMPILER="${CXX:-g++}" \
    2>&1 | grep -E "^--|Using"

cmake --build examples/build/_cmake_shm --target shm_writer shm_reader \
    -j "$(nproc)" 2>&1 | tail -2

echo ""

# ── Step 2: Run writer ────────────────────────────────────────────────
echo "[3/5] Starting writer (duration=${WRITER_DURATION})..."
./examples/build/shm_writer --duration "${WRITER_DURATION}" 2>/dev/null &
WRITER_PID=$!

# Give the writer time to create the shm segment and publish initial data
sleep 1

# ── Step 3: Run reader ────────────────────────────────────────────────
echo "[4/5] Running reader (duration=${READER_DURATION})..."
READER_OUTPUT=$(mktemp)
./examples/build/shm_reader --duration "${READER_DURATION}" \
    2>/dev/null > "${READER_OUTPUT}" || true

# Wait for writer to finish
wait "${WRITER_PID}" 2>/dev/null || true

# ── Step 4: Verify ────────────────────────────────────────────────────
echo "[5/5] Verifying data exchange..."
echo ""

TOTAL_LINES=$(wc -l < "${READER_OUTPUT}")
# Count non-zero samples (filter out lines containing only zeros)
# dump_block output format: "0.866025 ... -0.793353"
NONZERO_LINES=$(grep -cvE '^\s*0(\.0+)?\s+\.\.\.\s+0(\.0+)?\s*$' "${READER_OUTPUT}" 2>/dev/null || echo "0")

echo "  Reader received ${TOTAL_LINES} total lines"
echo "  Non-zero samples: ${NONZERO_LINES}"
echo ""

if [ "${TOTAL_LINES}" -gt 0 ] && [ "${NONZERO_LINES}" -gt 0 ]; then
    echo "  PASS: Data successfully exchanged via shared memory!"
    echo ""
    echo "  First 10 samples:"
    head -10 "${READER_OUTPUT}" | sed 's/^/    /'
else
    echo "  FAIL: No data received from shared memory."
    echo "  Check that the writer created the shm segment before the reader attached."
    rm -f "${READER_OUTPUT}"
    exit 1
fi

rm -f "${READER_OUTPUT}"

# Clean up shm segment (in case writer didn't unlink)
rm -f /dev/shm/pipit_demo 2>/dev/null || true

echo ""
echo "Done."
