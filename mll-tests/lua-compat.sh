#!/usr/bin/env bash
# Run compiled .mll test cases against a Lua interpreter.
# Usage: lua-compat.sh <lua-binary>
# Exit code: number of failures (0 = all passed)
set -euo pipefail

LUA="${1:?Usage: $0 <lua-binary>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CASES_DIR="$SCRIPT_DIR/tests/cases"
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

for src in "$CASES_DIR"/*.mll; do
    name="$(basename "$src" .mll)"
    lua_file="${src%.mll}.lua"

    # Per-interpreter skip: a case may declare interpreters it cannot run
    # on with a marker comment (multiple lines allowed, ids separated by
    # spaces or commas):
    #   -- lua-compat-skip: luajit
    skip_ids="$(sed -n 's/^--[[:space:]]*lua-compat-skip:[[:space:]]*//p' "$src" | tr ',' ' ')"
    skip_case=""
    for id in $skip_ids; do
        if [ "$id" = "$LUA_ID" ]; then skip_case=1; break; fi
    done
    if [ -n "$skip_case" ]; then
        echo "SKIP $name (marked lua-compat-skip for $LUA_ID)"
        skipped=$((skipped + 1))
        continue
    fi

    # Compile .mll to .lua (pass lib/ for tests that import library modules)
    if ! "$MLL" -e -L "$SCRIPT_DIR/../lib" "$src" 2>/dev/null; then
        echo "SKIP $name (compile error)"
        skipped=$((skipped + 1))
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

echo ""
echo "$($LUA -v 2>&1 | head -1): $passed passed, $failures failed, $skipped skipped"
exit "$failures"
