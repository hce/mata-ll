#!/usr/bin/env bash
# Run the compiled test corpus against a real Lua interpreter.
#   tests/cases/*.mll — compile and run; a case checks itself with `assert`,
#                       so a non-zero exit is the failure signal.
#   tests/ghc/*.mll   — compile, run, and compare stdout byte-for-byte with
#                       the pinned GHC golden (tests/ghc-golden/ghc/<name>.stdout)
#                       or, for a recorded divergence, with the pinned mata-ll
#                       output (tests/ghc-golden/divergent/ghc/<name>.stdout) —
#                       the same rule mll-tests/tests/run_mll/ghc_oracle.rs
#                       applies in-process.
# A compile error is a FAILURE: every corpus program compiles under
# `cargo test`, so one failing here means the mll binary is stale or broken.
# Usage: lua-compat.sh <lua-binary>
# Exit code: number of failures (0 = all passed)
set -euo pipefail

LUA="${1:?Usage: $0 <lua-binary>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CASES_DIR="$SCRIPT_DIR/tests/cases"
GHC_DIR="$SCRIPT_DIR/tests/ghc"
GOLDEN_DIR="$SCRIPT_DIR/tests/ghc-golden"
MLL="${MLL:-}"
if [ -z "$MLL" ]; then
    for candidate in "$SCRIPT_DIR/../target/release/mll" "$SCRIPT_DIR/../target/debug/mll"; do
        if [ -x "$candidate" ]; then MLL="$candidate"; break; fi
    done
fi
if [ -z "$MLL" ] || [ ! -x "$MLL" ]; then
    echo "Error: mll binary not found. Run 'cargo build' first."
    exit 1
fi

# Canonical interpreter id for per-case skip markers. Derived from the
# interpreter's own version string, not the binary name: CI invokes both
# 5.4 and 5.5 as plain `lua` inside a nix shell, so the name alone cannot
# tell them apart.
LUA_VERSION="$("$LUA" -v 2>&1 | head -1)"
case "$LUA_VERSION" in
    LuaJIT*) LUA_ID="luajit" ;;
    Lua\ *)  LUA_ID="lua$(printf '%s' "$LUA_VERSION" | sed 's/^Lua \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/')" ;;
    *)       LUA_ID="unknown" ;;
esac

failures=0
passed=0
skipped=0

# Per-interpreter skip: a case may declare interpreters it cannot run on
# with a marker comment (multiple lines allowed, ids separated by spaces or
# commas), followed by comment lines giving the reason:
#   -- lua-compat-skip: luajit
# Prints the SKIP line and returns 0 when the case is to be skipped.
skip_marked() {
    local src="$1" name="$2"
    local skip_ids id
    skip_ids="$(sed -n 's/^--[[:space:]]*lua-compat-skip:[[:space:]]*//p' "$src" | tr ',' ' ')"
    for id in $skip_ids; do
        if [ "$id" = "$LUA_ID" ]; then
            echo "SKIP $name (marked lua-compat-skip for $LUA_ID)"
            return 0
        fi
    done
    return 1
}

# Compile .mll to the sibling .lua (lib/ on the search path for tests that
# import library modules). Returns 1 on a compile error, after reporting it.
compile_case() {
    local src="$1" name="$2"
    if ! "$MLL" -e -L "$SCRIPT_DIR/../lib" "$src" >/dev/null 2>&1; then
        echo "FAIL $name (compile error)"
        return 1
    fi
    return 0
}

# --- tests/cases: self-asserting programs ---------------------------------
for src in "$CASES_DIR"/*.mll; do
    name="$(basename "$src" .mll)"
    lua_file="${src%.mll}.lua"

    if skip_marked "$src" "$name"; then
        skipped=$((skipped + 1))
        continue
    fi
    if ! compile_case "$src" "$name"; then
        failures=$((failures + 1))
        continue
    fi

    # Run under the target Lua interpreter
    if "$LUA" "$lua_file" >/dev/null 2>&1; then
        passed=$((passed + 1))
    else
        echo "FAIL $name ($LUA)"
        failures=$((failures + 1))
    fi

    rm -f "$lua_file"
done

# --- tests/ghc: stdout against the pinned goldens ---------------------------
actual_out="$(mktemp)"
trap 'rm -f "$actual_out"' EXIT
for src in "$GHC_DIR"/*.mll; do
    name="$(basename "$src" .mll)"
    lua_file="${src%.mll}.lua"

    if skip_marked "$src" "ghc/$name"; then
        skipped=$((skipped + 1))
        continue
    fi
    if ! compile_case "$src" "ghc/$name"; then
        failures=$((failures + 1))
        continue
    fi

    golden="$GOLDEN_DIR/ghc/$name.stdout"
    divergent="$GOLDEN_DIR/divergent/ghc/$name.stdout"
    if [ ! -f "$golden" ]; then
        # Outside the oracle's domain (regenerate-ghc-goldens.sh lists why,
        # e.g. HashMap builtins): the case runs self-asserting, like tests/cases.
        if "$LUA" "$lua_file" >/dev/null 2>&1 </dev/null; then
            passed=$((passed + 1))
        else
            echo "FAIL ghc/$name ($LUA)"
            failures=$((failures + 1))
        fi
        rm -f "$lua_file"
        continue
    fi
    # A recorded divergence pins mata-ll's own output; otherwise the GHC
    # golden is the expectation.
    expected="$golden"
    if [ -f "$divergent" ]; then expected="$divergent"; fi

    if "$LUA" "$lua_file" >"$actual_out" 2>/dev/null </dev/null \
        && cmp -s "$actual_out" "$expected"; then
        passed=$((passed + 1))
    else
        echo "FAIL ghc/$name ($LUA): stdout differs from $(basename "$(dirname "$(dirname "$expected")")")/ghc/$name.stdout"
        failures=$((failures + 1))
    fi

    rm -f "$lua_file"
done

echo ""
echo "$LUA_VERSION: $passed passed, $failures failed, $skipped skipped"
exit "$failures"
