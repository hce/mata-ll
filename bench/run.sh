#!/usr/bin/env bash
# Speed-of-light benchmark suite.
#
# Every workload exists twice: cases/<name>.mll (idiomatic mata-ll) and
# twins/<name>.lua (the same job handwritten the way a Lua programmer
# would do it). Both are timed with os.clock (CPU time, min over
# BENCH_RUNS separate processes) and the report shows the ratio
# mll/twin — how far the generated code is from the speed of light for
# that workload. The two programs must print byte-identical stdout; a
# twin that drifts from its workload fails the run instead of skewing
# the ratio.
#
# Usage: run.sh [lua-binary ...]          (default: lua5.5)
#   MLL=/path/to/mll   compiler override (default: target/release, then
#                      target/debug)
#   BENCH_RUNS=N       timed runs per program, minimum taken (default 3)
#   BENCH_ONLY=name    run a single workload
#
# Compiled output goes to a temp dir; bench/ stays clean.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
RUNS="${BENCH_RUNS:-3}"

MLL="${MLL:-}"
if [ -z "$MLL" ]; then
    for candidate in "$ROOT/target/release/mll" "$ROOT/target/debug/mll"; do
        if [ -x "$candidate" ]; then MLL="$candidate"; break; fi
    done
fi
if [ -z "$MLL" ] || [ ! -x "$MLL" ]; then
    echo "Error: mll binary not found. Run 'cargo build --release' first." >&2
    exit 1
fi

if [ "$#" -gt 0 ]; then LUAS=("$@"); else LUAS=(lua5.5); fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cat > "$WORK/timer.lua" <<'EOF'
local f = ...
local t0 = os.clock()
dofile(f)
local t1 = os.clock()
io.stderr:write(string.format("%.6f", t1 - t0))
EOF

# time_program <lua> <file> <expected-stdout-file-or-"">
# Prints the min os.clock time over $RUNS runs; verifies stdout when an
# expected file is given, else records stdout to $WORK/expected.
time_program() {
    local lua="$1" file="$2" expected="$3"
    local best="" t
    for _ in $(seq "$RUNS"); do
        if ! "$lua" "$WORK/timer.lua" "$file" > "$WORK/stdout" 2> "$WORK/time"; then
            echo "Error: $lua failed on $file:" >&2
            cat "$WORK/stdout" "$WORK/time" >&2
            exit 1
        fi
        t="$(cat "$WORK/time")"
        case "$t" in
            [0-9]*.[0-9]*) ;;
            *) echo "Error: no timing from $file (stderr: $t)" >&2; exit 1 ;;
        esac
        if [ -z "$best" ] || awk -v a="$t" -v b="$best" 'BEGIN { exit !(a < b) }'; then
            best="$t"
        fi
    done
    if [ -n "$expected" ]; then
        if ! cmp -s "$WORK/stdout" "$expected"; then
            echo "Error: twin output differs from mll output for $file" >&2
            diff "$expected" "$WORK/stdout" >&2 || true
            exit 1
        fi
    else
        cp "$WORK/stdout" "$WORK/expected"
    fi
    echo "$best"
}

CASES=()
for f in "$SCRIPT_DIR"/cases/*.mll; do
    name="$(basename "$f" .mll)"
    if [ -n "${BENCH_ONLY:-}" ] && [ "$name" != "$BENCH_ONLY" ]; then continue; fi
    CASES+=("$name")
    if [ ! -f "$SCRIPT_DIR/twins/$name.lua" ]; then
        echo "Error: no twin for $name (twins/$name.lua missing)" >&2
        exit 1
    fi
    cp "$f" "$WORK/$name.mll"
    "$MLL" -e "$WORK/$name.mll"
done
if [ "${#CASES[@]}" -eq 0 ]; then
    echo "Error: no workloads matched" >&2
    exit 1
fi

for lua in "${LUAS[@]}"; do
    echo "== $("$lua" -v 2>&1 | head -1) =="
    printf '%-16s %10s %10s %8s\n' workload "mll(s)" "twin(s)" ratio
    for name in "${CASES[@]}"; do
        mll_t="$(time_program "$lua" "$WORK/$name.lua" "")"
        twin_t="$(time_program "$lua" "$SCRIPT_DIR/twins/$name.lua" "$WORK/expected")"
        awk -v n="$name" -v a="$mll_t" -v b="$twin_t" \
            'BEGIN { printf "%-16s %10.3f %10.3f %7.1fx\n", n, a, b, (b > 0) ? a / b : -1 }'
    done
    echo
done
