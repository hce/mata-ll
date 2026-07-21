#!/bin/sh
# regenerate-ghc-goldens.sh — pin real GHC's output for the parity corpus.
#
# For every oracle-eligible test case in tests/cases/ and tests/ghc/ this
# script builds a mechanical GHC twin (the unchanged .mll source, prefixed
# with a header that imports the shared shim tests/ghc-golden/MllShim.hs),
# runs it under runghc, and stores its stdout as the golden file
# tests/ghc-golden/{cases,ghc}/<name>.stdout.
#
# The goldens are committed artifacts: CI and `cargo test` never need GHC.
# Re-run this script (on a machine with GHC in PATH or ~/.ghcup/bin) only to
# re-pin after adding cases or changing the shim. mata-ll's runtime output is
# diffed against these files by the ghc_oracle_* tests in tests/run_mll.rs.
#
# Cases that cannot be twinned are excluded here, each with a recorded
# reason (see `excluded_reason` below). Cases whose mata-ll output is KNOWN
# to differ from GHC's are still goldened; the differences are pinned in
# tests/ghc-golden/divergent/ and listed in tests/ghc-golden/DIVERGENCES.md.
# Cases whose divergence aborts the GHC twin itself (an assert that encodes
# mata-ll's output format fails under GHC) cannot be goldened at all; they
# are excluded with reason "diverges:" and documented in DIVERGENCES.md.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
tests=$here/tests
gold=$tests/ghc-golden

# --- locate runghc -----------------------------------------------------------
RUNGHC=${RUNGHC:-}
if [ -z "$RUNGHC" ]; then
    if command -v runghc >/dev/null 2>&1; then RUNGHC=runghc
    elif [ -x "$HOME/.ghcup/bin/runghc" ]; then RUNGHC=$HOME/.ghcup/bin/runghc
    else
        echo "error: runghc not found (PATH, ~/.ghcup/bin); set RUNGHC=" >&2
        exit 1
    fi
fi
echo "using $($RUNGHC --version)"

# --- exclusion table ---------------------------------------------------------
# Prints the reason a case is excluded from the oracle corpus; empty output
# means the case is eligible. Keep DIVERGENCES.md in sync for diverges: rows.
excluded_reason() {
    case "$1" in
        # -- FFI / Lua-runtime surfaces: not expressible under GHC ------------
        cases/any_type)                  echo "builtin Any (Lua dynamic value carrier)";;
        cases/bytestring)                echo "ByteString builtins (Lua byte strings)";;
        cases/bytestring_u64_sign_bit)   echo "ByteString + Lua 64-bit wrap-around semantics";;
        cases/constructor_as_rename)     echo "'as \"name\"' constructor-rename syntax (JSON FFI, not Haskell)";;
        cases/constructor_shadowing_json) echo "JSON FFI library";;
        cases/derive_fromjson)           echo "JSON FFI library";;
        cases/derive_tojson)             echo "JSON FFI library";;
        cases/error_forces_message)      echo "imports LString (Lua string FFI)";;
        cases/exitvalue_prelude)         echo "ExitValue/exit (Lua process control)";;
        cases/ffi)                       echo "FFI declarations";;
        cases/ffi_constructed_values)    echo "FFI declarations";;
        cases/ffi_maybe_args)            echo "FFI declarations";;
        cases/ffi_strictness)            echo "FFI declarations";;
        cases/getline)                   echo "reads stdin; LuaUserData FFI";;
        cases/hashmap)                   echo "HashMap builtins (Lua tables)";;
        cases/json_codec)                echo "JSON FFI library";;
        cases/lbit_64bit_boundary)       echo "LBit (Lua bit-op semantics, deliberately not Data.Bits)";;
        cases/lbit_strict_primitive_arg) echo "LBit (Lua bit-op semantics)";;
        cases/lib_json)                  echo "JSON FFI library";;
        cases/lib_lbit)                  echo "LBit (Lua bit-op semantics)";;
        cases/lib_liolinear)             echo "LIOLinear file FFI + linear types";;
        cases/lib_lmath)                 echo "LMath FFI (Lua math library)";;
        cases/lib_los)                   echo "LOS FFI (clock/env/tmpname: host-dependent)";;
        cases/lib_lstring)               echo "LString FFI (Lua string library)";;
        cases/lib_regex)                 echo "Regex FFI (Lua patterns)";;
        cases/lua_iterator_method)       echo "LuaIterator FFI";;
        cases/luacatch)                  echo "LuaCatch FFI";;
        cases/luadict)                   echo "LuaDict FFI";;
        cases/poly_recursion_user_class) echo "contains a LuaPure FFI declaration";;
        cases/purehashmap)               echo "LuaPure hash FFI";;
        cases/readline)                  echo "LIO.readLine (Lua io FFI)";;
        cases/export_module)             echo "'export' keyword (Lua module export)";;
        cases/lib_data_map)              echo "mata-ll Data.Map API (values/mapPairs/...) is its own library, not containers";;
        ghc/ghc_regr005)                 echo "HashMap builtins (Lua tables)";;

        # -- linear types: same surface syntax, different core discipline -----
        # Under -XLinearTypes GHC makes constructor fields linear by default
        # and forbids passing a %1-bound scalar to an unrestricted function
        # (e.g. `useOnce (Token n) = n * 2` is GHC-18872); mata-ll counts such
        # a use as the one consumption. The twins are compile-time rejected.
        cases/linear_affine_basic)       echo "GHC's -XLinearTypes rejects unrestricted use of a %1-bound scalar";;
        cases/linear_mult_poly)          echo "GHC's -XLinearTypes rejects unrestricted use of a %1-bound scalar";;

        # -- mata-ll-only grammar / name shadowing ----------------------------
        cases/newtypes)                  echo "'newtype Age = Integer' implicit-constructor sugar is not Haskell";;
        cases/superclass)                echo "redefines the Eq/Ord classes (mata-ll builtin shadowing)";;
        cases/constructor_shadowing)     echo "redefines Just/Nothing constructors (Prelude shadowing)";;
        cases/lua_keywords)              echo "record fields named after Lua keywords collide with Prelude.until under GHC";;

        # -- helper modules, not runnable cases -------------------------------
        cases/DiamondLeaf)               echo "helper module (twinned for import, no main)";;
        cases/DiamondMid)                echo "helper module (twinned for import, no main)";;
        cases/ExportHelper)              echo "helper module (twinned for import, no main)";;

        # -- known divergences that abort the GHC twin ------------------------
        # An assert in the case encodes mata-ll's output format; under GHC the
        # assert fails, so no golden can exist. Details: DIVERGENCES.md.
        cases/edge_cases)                echo "diverges: asserts show \"\" == \"\" (GHC: \"\\\"\\\"\")";;
        cases/haskell_compat)            echo "diverges: asserts show [1,2,3] == \"[1, 2, 3]\"";;
        cases/instance_context)          echo "diverges: asserts show (Leaf \"s\") == \"Leaf s\"";;
        cases/instance_context_multi)    echo "diverges: asserts show (Pair 1 \"hi\") == \"Pair 1 hi\"";;
        cases/instance_context_paren)    echo "diverges: asserts show (Wrap (Wrap \"x\")) == \"Wrap Wrap x\"";;
        cases/lazy_index_thunk_leak)     echo "diverges: asserts show (1, True) == \"(1, True)\"";;
        cases/mangle_collision)          echo "diverges: asserts show (MkAB, MkC) == \"(MkAB, MkC)\"";;
        cases/pair_ord_fields)           echo "diverges: asserts show of nested Pair without string quotes";;
        cases/poly_recursion)            echo "diverges: asserts list show with \", \" spacing";;
        cases/show_either)               echo "diverges: asserts show (Left \"x\") == \"Left x\"";;
        cases/tuple_field_laziness)      echo "diverges: asserts tuple show with \", \" spacing";;
        cases/typeclasses_full)          echo "diverges: asserts show (Circle 3.0) == \"Circle 3\"";;
        cases/unit_type)                 echo "diverges: asserts show [(), ()] == \"[(), ()]\"";;
        ghc/ghc_cgrun030)                echo "diverges: asserts show [1,2,3] == \"[1, 2, 3]\"";;
        ghc/ghc_regr003)                 echo "diverges: asserts show \"red\" == \"red\" (unquoted)";;
        *) : ;;
    esac
}

# --- twin generation ---------------------------------------------------------
gen=$(mktemp -d "${TMPDIR:-/tmp}/mll-ghc-goldens.XXXXXX")
trap 'rm -rf "$gen"' EXIT

pragmas='{-# LANGUAGE GHC2021, GADTs, DataKinds, TypeFamilies, UndecidableInstances, OverloadedRecordDot, LinearTypes #-}'
hiding='import Prelude hiding (length, take, drop, replicate, (!!), (==), (/=), (<), (<=), (>), (>=), elem, (<$>), (<*>))'
# The qualified Prelude import keeps hidden class methods ((==), (<$>), ...)
# visible so test cases can still bind them in instance declarations.
qualprelude='import qualified Prelude as GhcPrelude'

# Rewrite the body of a case for GHC:
#  * Data.List / Data.Foldable also export the names the shim redefines at
#    Integer types; hide (bare import) or strip (explicit import list) them
#    so the shim's definitions are unambiguous. Purely mechanical; the
#    imported functions have the same semantics.
rewrite_body() {
    awk '
    /^import Data\.List *$/     { print "import Data.List hiding (length, take, drop, replicate, (!!), elem)"; print "import MllShimDataList"; next }
    /^import Data\.Foldable *$/ { print "import Data.Foldable hiding (length, elem)"; next }
    /^import (Data\.List|Data\.Foldable) *\(/ {
        # strip shim-owned names from an explicit import list
        for (n = split("length take drop replicate (!!) elem", names, " "); n >= 1; n--) {
            gsub(", " names[n] ",", ",");   # middle
            gsub("\\(" names[n] ", ", "("); # first
            gsub(", " names[n] "\\)", ")"); # last
            gsub("\\(" names[n] "\\)$", "()"); # only
        }
        print; next
    }
    { print }
    '
}

# Build the twin for one file. Main-program files get a synthetic
# `module Main where` header; helper-module files (those with their own
# `module X (...) where` line) keep it, and the shim imports are inserted
# directly after it.
make_twin() { # $1 = source .mll, $2 = output .hs
    if grep -q '^module ' "$1"; then
        {
            printf '%s\n' "$pragmas"
            awk -v hiding="$hiding" -v qualprelude="$qualprelude" '
                { print }
                /^module .* where *$/ && !done {
                    print hiding; print qualprelude; print "import MllShim"; done = 1
                }
            ' "$1" | rewrite_body
        } > "$2"
    else
        {
            printf '%s\nmodule Main where\n%s\n%s\nimport MllShim\n' \
                "$pragmas" "$hiding" "$qualprelude"
            rewrite_body < "$1"
        } > "$2"
    fi
}

# Helper modules must be importable by name from the twin directory.
for helper in DiamondLeaf DiamondMid ExportHelper; do
    make_twin "$tests/cases/$helper.mll" "$gen/$helper.hs"
done

# --- run the corpus ----------------------------------------------------------
rm -rf "$gold/cases" "$gold/ghc"
mkdir -p "$gold/cases" "$gold/ghc"

total=0; pinned=0; skipped=0; failed=0
failures=""

for sub in cases ghc; do
    for f in "$tests/$sub"/*.mll; do
        name=$(basename "$f" .mll)
        total=$((total + 1))
        reason=$(excluded_reason "$sub/$name")
        if [ -n "$reason" ]; then
            skipped=$((skipped + 1))
            continue
        fi
        hs=$gen/${sub}_${name}.hs
        make_twin "$f" "$hs"
        out=$gold/$sub/$name.stdout
        # 120s guard against accidental non-termination (perl: no macOS timeout(1))
        if perl -e 'alarm 120; exec @ARGV' -- \
                "$RUNGHC" -i"$gen" -i"$gold" "$hs" > "$out" 2> "$gen/err"; then
            pinned=$((pinned + 1))
        else
            failed=$((failed + 1))
            failures="$failures $sub/$name"
            rm -f "$out"
            echo "FAIL $sub/$name:" >&2
            sed 's/^/    /' "$gen/err" | head -12 >&2
        fi
    done
done

echo "corpus: $total files; goldens pinned: $pinned; excluded: $skipped; failed: $failed"
if [ -n "$failures" ]; then
    echo "unexpected failures (fix the shim, or exclude with a recorded reason):$failures" >&2
    exit 1
fi
