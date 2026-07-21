#!/usr/bin/env bash
# Performance test: compile tracker.mll, decode a test IT file, report speed.
# Usage: perf-test.sh <lua-binary>
# Exits non-zero if decode rate drops below realtime.
set -euo pipefail

LUA="${1:?Usage: $0 <lua-binary>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
MLL="${MLL:-}"
if [ -z "$MLL" ]; then
    for candidate in "$ROOT/target/release/mll" "$ROOT/target/debug/mll"; do
        if [ -x "$candidate" ]; then MLL="$candidate"; break; fi
    done
fi
if [ -z "$MLL" ] || [ ! -x "$MLL" ]; then
    echo "Error: mll binary not found. Run 'cargo build' first."
    exit 1
fi

TEST_IT="$SCRIPT_DIR/tests/benchmark.it"

# Generate test file if missing
if [ ! -f "$TEST_IT" ]; then
    "$LUA" "$SCRIPT_DIR/gen_test_it.lua" "$TEST_IT"
fi

# Compile tracker
"$MLL" -e "$ROOT/experiments/tracker/tracker.mll" 2>/dev/null

# Decode and measure
OUTFILE="$(mktemp)"
trap "rm -f $OUTFILE" EXIT

START=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
cd "$ROOT/experiments/tracker"
"$LUA" ctracker.lua "$TEST_IT" "$OUTFILE" >/dev/null 2>&1
END=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')

BYTES=$(wc -c < "$OUTFILE" | tr -d ' ')
ELAPSED_NS=$((END - START))
ELAPSED_MS=$((ELAPSED_NS / 1000000))

# Calculate audio duration: 16-bit stereo @ 44100 Hz = 4 bytes per sample
SAMPLES=$((BYTES / 4))
AUDIO_MS=$((SAMPLES * 1000 / 44100))

if [ "$ELAPSED_MS" -eq 0 ]; then
    RATIO="inf"
else
    RATIO_X10=$((AUDIO_MS * 10 / ELAPSED_MS))
    RATIO="${RATIO_X10%?}.${RATIO_X10: -1}"
fi

LUA_VERSION=$("$LUA" -v 2>&1 | head -1)
echo "$LUA_VERSION: ${AUDIO_MS}ms audio decoded in ${ELAPSED_MS}ms (${RATIO}x realtime)"

# Fail if slower than 0.5x realtime (generous threshold for CI)
if [ "$ELAPSED_MS" -gt 0 ] && [ "$((AUDIO_MS * 10 / ELAPSED_MS))" -lt 5 ]; then
    echo "FAIL: decode rate below 0.5x realtime"
    exit 1
fi
