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

failures=0
passed=0
skipped=0

for src in "$CASES_DIR"/*.mll; do
    name="$(basename "$src" .mll)"
    lua_file="${src%.mll}.lua"

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
