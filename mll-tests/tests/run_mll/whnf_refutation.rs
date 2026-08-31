//! Self-tests for the WHNF-refutation mode (`compile_with_whnf_refutation`):
//! the claim checkers must FIRE on a violation — a harness whose checks can
//! never fail proves nothing. The compiler is expected to emit no violations,
//! so the trip-wires are exercised by APPENDING a tampering snippet to
//! instrumented output: the emitted file's runtime definitions (`__thunk`,
//! `__force`, `__assert_whnf`, …) are chunk-level locals and stay in scope at
//! the end of the chunk, so appended Lua can drive them directly with
//! hand-built violations.

use super::*;
use std::path::Path;

/// A program whose instrumented output is guaranteed to reference every
/// definition the tampering snippets drive: `nums`' lazy spine keeps
/// `__thunk`/`__force` selected; `halve`'s `div` in a forced operand
/// position is an `infix_yields_whnf` claim, so it references
/// `__assert_whnf`; and `mk`'s `pure` of a literal is a bare escape
/// (`pure_value_bare_is_safe`), so it references `__assert_purebare`.
/// `checkers_present_and_program_still_runs` pins that guarantee so the
/// other tests cannot silently drive missing names.
const PROGRAM: &str = "\
nums :: [Int]
nums = map (\\x -> x + 1) [1, 2, 3]

halve :: Int -> Int
halve n = n `div` 2 + 1

mk :: Int -> IO Int
mk _ = pure 42

main :: IO ()
main = do
    v <- mk 1
    print v
    -- halve's argument is runtime-computed: a constant argument would let
    -- the fold pass evaluate the div away, and the claim site with it.
    print (halve (sum nums))
";

/// Instrumented (whnf-refutation) output for `PROGRAM` plus `snippet`
/// appended at chunk level, with `print` neutralized so the program's own
/// output stays out of the test log.
fn instrumented_with(snippet: &str) -> String {
    let code = with_compiler_stack(|| {
        mllc::compile_with_whnf_refutation(PROGRAM, Path::new("."), &[])
    })
    .expect("program should compile")
    .lua_code;
    format!("print = function() end\n{code}\n{snippet}")
}

fn run_lua(code: &str) -> Result<(), mlua::Error> {
    mlua::Lua::new().load(code).exec()
}

/// The instrumentation is present, the rebind is in place, and the program
/// still runs with its observable behavior intact (the corpus second pass
/// re-checks this for every case; this pins the minimal example the other
/// tests build on).
#[test]
fn checkers_present_and_program_still_runs() {
    let code = instrumented_with("");
    for needle in [
        "__force = __force_checked",
        "__force_checked",
        "__assert_whnf",
        "__assert_purebare",
        "__thunk",
    ] {
        assert!(
            code.contains(needle),
            "instrumented output must reference {needle:?} (the trip-wire \
             tests drive it); adjust PROGRAM if a codegen change dropped it"
        );
    }
    run_lua(&code).expect("instrumented program must run cleanly");
}

/// The instrumentation fires broadly, not just on the minimal example: a
/// nontrivial corpus case must carry several claim checks. This pins the
/// refutation mode against accidental neutering — a skip-list widened to
/// everything would keep every test green while checking nothing.
#[test]
fn instrumentation_covers_a_nontrivial_case() {
    let path = Path::new("tests/cases/over_application_arity.mll");
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let code = with_compiler_stack(|| {
        mllc::compile_with_whnf_refutation(&source, Path::new("tests/cases"), &[])
    })
    .expect("case should compile")
    .lua_code;
    let hits = code.matches("__assert_whnf(").count();
    assert!(
        hits >= 3,
        "expected several __assert_whnf claim checks in a nontrivial case, found {hits}"
    );
}

/// Production output must carry none of the instrumentation: the checkers
/// exist only behind the refutation entry point.
#[test]
fn production_output_carries_no_checkers() {
    let code = with_compiler_stack(|| {
        mllc::compile(PROGRAM, Path::new("."), &[])
    })
    .expect("program should compile")
    .lua_code;
    for needle in ["__assert_whnf", "__assert_purebare", "__force_checked"] {
        assert!(
            !code.contains(needle),
            "production output must not mention {needle:?}"
        );
    }
}

/// `__assert_whnf` rejects an unforced thunk and passes a plain value through.
#[test]
fn assert_whnf_trips_on_a_thunk() {
    let code = instrumented_with(
        "local ok, err = pcall(function() \
             return __assert_whnf(__thunk(function() return 1 end)) end)\n\
         assert(not ok and tostring(err):find(\"WHNF claim refuted\"), \
             \"assert_whnf must reject an unforced thunk\")\n\
         assert(__assert_whnf(5) == 5, \"assert_whnf must pass a value through\")\n",
    );
    run_lua(&code).expect("trip-wire snippet must pass");
}

/// The checked `__force` (rebound over the standard one) rejects a thunk body
/// that returns a raw thunk — the one-level force invariant — while still
/// forcing and memoizing an honest thunk.
#[test]
fn checked_force_trips_on_a_nested_thunk() {
    let code = instrumented_with(
        "local nested = __thunk(function() \
             return __thunk(function() return 1 end) end)\n\
         local ok, err = pcall(function() return __force(nested) end)\n\
         assert(not ok and tostring(err):find(\"force invariant refuted\"), \
             \"checked force must reject a thunk body returning a raw thunk\")\n\
         local t = __thunk(function() return 7 end)\n\
         assert(__force(t) == 7 and __force(t) == 7, \
             \"checked force must still force and memoize\")\n",
    );
    run_lua(&code).expect("trip-wire snippet must pass");
}

/// `__assert_purebare` rejects both halves of the bare-escape claim — a thunk
/// (forcing is not a no-op) and a Lua function (`__mll_run`'s action test
/// would wrongly call it) — and passes a plain value through.
#[test]
fn assert_purebare_trips_on_function_and_thunk() {
    let code = instrumented_with(
        "local ok1 = pcall(function() \
             return __assert_purebare(function() end) end)\n\
         local ok2 = pcall(function() \
             return __assert_purebare(__thunk(function() return 1 end)) end)\n\
         assert(not ok1, \"assert_purebare must reject a Lua function\")\n\
         assert(not ok2, \"assert_purebare must reject a thunk\")\n\
         assert(__assert_purebare(9) == 9, \
             \"assert_purebare must pass a value through\")\n",
    );
    run_lua(&code).expect("trip-wire snippet must pass");
}
