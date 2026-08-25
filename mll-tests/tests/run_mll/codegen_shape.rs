//! Emitted-Lua inspection tests: determinism, erasure/inlining/DCE shape,
//! paren normalization, provenance stamps, warnings, source embedding, and
//! canonical string escaping.

use super::*;
use crate::ffi::compile_ffi_module;

/// Code generation must be deterministic: the same source compiled repeatedly
/// must produce byte-identical Lua. (Regression guard for HashMap iteration
/// order leaking into emission — record accessors, FFI functions, and
/// specialization resolution were all order-sensitive.)
#[test]
fn codegen_is_deterministic() {
    let source = r#"
data Color = Red | Green | Blue deriving (Show, Eq, Ord)
data Person = Person { pName :: String, pAge :: Int, pCity :: String, pActive :: Bool }
data Tree a = Leaf a | Branch (Tree a) (Tree a) deriving (Show, Eq)

class Describe a where
  describe :: a -> String
instance Describe Color where
  describe Red = "red"
  describe Green = "green"
  describe Blue = "blue"

mapList :: (a -> b) -> [a] -> [b]
mapList _ [] = []
mapList f (x:xs) = f x : mapList f xs

depth :: Tree a -> Int
depth (Leaf _) = 0
depth (Branch l r) = 1 + max (depth l) (depth r)

main :: IO ()
main = do
  let p = Person { pName = "Ann", pAge = 30, pCity = "NYC", pActive = True }
  putStr (pName p)
  putStrLn (pCity p)
  putStrLn (show (mapList (\n -> n + 1) [1, 2, 3]))
  putStrLn (show (mapList (\s -> s) ["a", "b"]))
  putStrLn (show (depth (Branch (Leaf 1) (Leaf 2))))
  putStrLn (describe Blue)
  putStrLn (show (compare Red Blue))
  let m = hmFromList [("a", 1), ("b", 2)]
  putStrLn (show (hmSize m))
"#;
    let dir = Path::new("tests/cases");
    let first = compile(source, dir, &[])
        .expect("compile should succeed")
        .lua_code;
    for i in 1..8 {
        let again = compile(source, dir, &[])
            .expect("compile should succeed")
            .lua_code;
        assert!(
            again == first,
            "codegen is non-deterministic: compile #{} differs from #0",
            i
        );
    }
}

/// The numeric classes (Num/Fractional/Integral) must be fully ERASED at
/// concrete Int/Number types: `+ - * /` emit bare Lua operators, `div`/`mod`
/// emit the existing strict cores, and NO dictionary is passed and NO named
/// instance helper (fromInteger_Int, plus_*, a dict table) is materialised.
/// This is the byte-identity guarantee for concrete arithmetic — the hot path
/// (e.g. the tracker mixer) must not gain an allocation or an indirection.
#[test]
fn numeric_classes_erased_at_concrete_types() {
    // `opaque` is a FOLD BARRIER: its body contains a call, so the TIR
    // constant folder neither beta-reduces it nor sees literal arguments
    // at the probed call sites — the arithmetic bodies must survive to
    // codegen (all-literal calls would fold to literals and the bodies
    // would be dead code).
    let source = r#"
opaque :: a -> a
opaque v = if length [] == 0 then v else v

hot :: Int -> Int -> Int
hot a b = a * b + a - b

frac :: Number -> Number -> Number
frac x y = x / y + x * y

flr :: Int -> Int -> Int
flr a b = (a `div` b) + (a `mod` b)

main :: IO ()
main = do
  putStrLn (show (hot (opaque 3) 4))
  putStrLn (show (frac (opaque 3.0) 4.0))
  putStrLn (show (flr (opaque 17) 5))
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;

    // Concrete Int/Number arithmetic inlines to bare Lua operators…
    assert!(lua.contains("(a * b)"), "Int * must inline to bare Lua *: {lua}");
    assert!(lua.contains("(x / y)"), "Number / must inline to bare Lua /: {lua}");
    // …div/mod stay on the existing strict cores…
    assert!(lua.contains("__mll_div("), "div must stay on __mll_div: {lua}");
    assert!(lua.contains("__mll_mod("), "mod must stay on __mll_mod: {lua}");
    // …and NOTHING dispatches through a Num/Fractional/Integral dictionary or a
    // per-type instance helper at these concrete types.
    for forbidden in [
        "fromInteger_Int", "fromInteger_Number", "fromRational_Number",
        "plus_Int", "times_Int", "__mll_dict", "__dict_Num",
    ] {
        assert!(
            !lua.contains(forbidden),
            "concrete arithmetic must not emit `{forbidden}` (no dictionary/instance dispatch): {lua}"
        );
    }
}

/// Call-site inlining must not duplicate argument work (sharing loss vs
/// GHC). `sq x = x * x` is an inline candidate; substitution re-emits the
/// argument at every occurrence of `x`, so a non-trivial argument (a call)
/// may only be substituted where the parameter occurs at most once —
/// otherwise the site falls back to the ordinary call and the argument is
/// evaluated exactly once. Distinctive literals mark each argument: the
/// literal's occurrence count in the emitted Lua IS the number of times
/// the argument was emitted.
#[test]
fn inlining_preserves_argument_sharing() {
    // `opaque` is a FOLD BARRIER (see numeric_classes_erased_at_concrete_types):
    // without it the all-literal probe calls constant-fold at TIR level and
    // never reach the call-site inliner under test.
    let source = r#"
opaque :: a -> a
opaque v = if length [] == 0 then v else v

sq :: Int -> Int
sq x = x * x

probe :: Int -> Int
probe n = n + 1

main :: IO ()
main = do
  let y = 6
  putStrLn (show (sq (opaque 90001)))
  putStrLn (show (sq y))
  putStrLn (show (probe (opaque 80002)))
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    // The non-trivial argument to the multiply-using `sq` is emitted ONCE
    // (before the fix: twice — `probe(90001) * probe(90001)`).
    assert_eq!(
        lua.matches("90001").count(),
        1,
        "argument to sq must be emitted exactly once (sharing): {lua}"
    );
    // The trivial-argument call still inlines: `sq y` becomes `y * y`.
    assert!(
        lua.contains("y * y"),
        "sq of a variable must still inline to y * y: {lua}"
    );
    // A once-used parameter (`probe n = n + 1`) still admits a non-trivial
    // argument, and that argument is emitted once.
    assert_eq!(
        lua.matches("80002").count(),
        1,
        "argument to probe must be emitted exactly once: {lua}"
    );
}

/// Big-integer literals are hoisted to a single `__mll_biglit` CAF table
/// (codegen/mod.rs BigLitPool): the decimal->bignum parse runs once at load,
/// not on every evaluation. Before the fix a literal in a loop body re-parsed
/// its decimal string every iteration, and two textual copies of one constant
/// parsed twice. The occurrence count of `__int_from_decimal("<digits>")` in
/// the emitted Lua IS the number of parses, so a distinct constant used many
/// times must appear exactly once.
#[test]
fn big_integer_literals_are_hoisted() {
    let source = r#"
big :: Integer
big = 340282366920938463463374607431768211456

loop :: Int -> Integer -> Integer
loop 0 acc = acc
loop n acc = loop (n - 1) (acc + 123456789012345678901234567890)

twice :: Integer
twice = 123456789012345678901234567890 + 123456789012345678901234567890

main :: IO ()
main = print (loop 3 big + twice)
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    // The pool table is emitted and indexed from the body.
    assert!(lua.contains("local __mll_biglit = {"), "biglit pool table missing: {lua}");
    assert!(lua.contains("__mll_biglit["), "body must index the biglit pool: {lua}");
    // Each distinct constant is parsed exactly once, despite the loop-body
    // literal being used once per iteration and the `twice` constant appearing
    // three times textually across the program.
    assert_eq!(
        lua.matches("__int_from_decimal(\"123456789012345678901234567890\")").count(),
        1,
        "the repeated/loop literal must be parsed once (hoisted): {lua}"
    );
    assert_eq!(
        lua.matches("__int_from_decimal(\"340282366920938463463374607431768211456\")").count(),
        1,
        "the top-level literal must be parsed once (hoisted): {lua}"
    );
    // No inline decimal parse survives in the loop function body: the parse
    // lives only in the pool table above.
    let pool_start = lua.find("local __mll_biglit = {").unwrap();
    let pool_end = lua[pool_start..].find("\n}\n").map(|i| pool_start + i).unwrap();
    let after_pool = &lua[pool_end..];
    assert!(
        !after_pool.contains("__int_from_decimal(\""),
        "no inline decimal-literal parse may survive past the pool: {after_pool}"
    );
}

/// Paren normalization, the KEEPS-paren side. The eliminated-paren side is
/// checked structurally per corpus program by the stamp refutation
/// (opt::run_refuted re-runs the expression passes over the final tree and
/// any change refutes — see expression_pass_idempotence), which replaced
/// the operator-list grep this test used to do. What idempotence cannot
/// express is a paren that must SURVIVE: an FFI wrapper truncates a
/// multi-returning host call to the declared single result, so the paren
/// there is semantics, not grouping.
#[test]
fn ffi_wrapper_keeps_truncating_paren() {
    let dir = Path::new("tests/cases");
    // A non-literal argument (the do-local `x` — fold propagates only
    // top-level CAFs) keeps the call out of the fold's splice, so the
    // wrapper survives to probe its own body shape.
    let ffi_source = "modf1 :: Number -> LuaPure \"math.modf\" Number\n\
                      main :: IO ()\n\
                      main = do\n\
                      \x20   let x = 3.75\n\
                      \x20   assert (modf1 x == 3.0) \"t\"\n";
    let lua = compile(ffi_source, dir, &[])
        .expect("ffi probe must compile")
        .lua_code;
    assert!(
        lua.contains("return (math.modf(__ffi0))"),
        "FFI wrapper must keep the truncating paren around the raw host call: {lua}"
    );
    // A literal argument splices the wrapper's SpecCall to the call site;
    // the truncating paren must ride along (the site is a single-value
    // position built from the same emission arm, but pin it explicitly).
    let spliced_source = "modf1 :: Number -> LuaPure \"math.modf\" Number\n\
                          main :: IO ()\n\
                          main = assert (modf1 3.75 == 3.0) \"t\"\n";
    let lua2 = compile(spliced_source, dir, &[])
        .expect("ffi probe must compile")
        .lua_code;
    assert!(
        lua2.contains("((math.modf(3.75)))"),
        "spliced FFI call must keep the truncating paren around the raw host call: {lua2}"
    );
}

/// Where-bound function-group names hold Lua function values from their
/// group assignment on — never thunks — and a `_warg` entry rebind leaves
/// the parameter WHNF. Neither may be re-forced (function.rs marks both
/// concrete; opt.rs pass 4 is the safety net).
#[test]
fn where_group_calls_not_forced() {
    let source = std::fs::read_to_string("tests/cases/where_group_mutual.mll")
        .expect("read where_group_mutual.mll");
    let lua = compile(&source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    for forbidden in ["__force(go)", "__force(swap)"] {
        assert!(
            !lua.contains(forbidden),
            "where-group emission must not re-force `{forbidden}`: {lua}"
        );
    }
    // `__force(_warg…)` may appear only as the entry rebind itself
    // (`_wargN = __force(_wargN)`), never as a re-force at a use site.
    for line in lua.lines().filter(|l| l.contains("__force(_warg")) {
        let t = line.trim_start();
        assert!(
            t.starts_with("_warg") && t.contains(" = __force(_warg"),
            "entry-rebound _warg param re-forced at a use site: {line}"
        );
    }
}

/// A parameter scrutinized only by LATER clauses is forced ONCE at the
/// chain split (`_wargN = __force(_wargN)` in the first clause's `else`),
/// not per use inside the later clauses' conditions: a deep cons pattern
/// paid one `__force` per spine step per attempt (salsa's lwGo re-forced
/// its list four times per iteration; huffman's go, its tree thrice).
#[test]
fn later_clause_param_forced_once_at_split() {
    let source = r#"
loadWords :: [Int] -> [Int]
loadWords bs = lw bs 0
  where
    lw _ 16 = []
    lw (b0 : b1 : b2 : b3 : rest) n = (b0 + b1 + b2 + b3) : lw rest (n + 1)
    lw _ _ = []

main :: IO ()
main = putStrLn (show (loadWords [1, 2, 3, 4, 5, 6, 7, 8]))
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    // Exactly one force of the list param: the split rebind itself. The
    // deep-cons clause condition and its bindings read the bare name.
    let forces: Vec<&str> = lua
        .lines()
        .filter(|l| l.contains("__force(_warg0)"))
        .collect();
    assert!(
        forces.len() == 1 && forces[0].trim_start().starts_with("_warg0 = __force(_warg0)"),
        "list param must be forced exactly once, at the split rebind: {lua}"
    );
    assert!(
        lua.contains("__mll_tail(_warg0)"),
        "the split clause's condition must read the rebound param bare: {lua}"
    );
}

/// A clause chain that KEEPS its non-exhaustive fall-off (literal-headed
/// cons defeats the coverage proof) must still convert to a loop: the
/// fall-off is the chain's `else` arm — not a trailing statement, which
/// kept every clause `return` out of statement-tree tail position — and a
/// `return error_(…)` fall-off clause passes the single-return proof
/// (error_ never returns at all). huffman's go was declined on both counts.
#[test]
fn tailloop_converts_chain_with_error_fall_off() {
    let source = r#"
walk :: Int -> [Int] -> Int
walk n bits = go n bits
  where
    go acc (0 : rest) = go acc rest
    go acc (b : rest) = go (acc + b) rest
    go _ [] = error "walk: ran out"

main :: IO ()
main = putStrLn (show (walk 0 [1, 0, 2, 0, 3]))
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    assert!(
        lua.contains("while true do"),
        "error-fall-off chain must convert to a loop: {lua}"
    );
    assert!(
        lua.contains("_warg0, _warg1 = "),
        "tail self-calls must become the loop's parameter update: {lua}"
    );
    assert!(
        lua.contains(r#"return error_("walk: ran out")"#),
        "the error clause stays a plain raising return: {lua}"
    );
}

/// A cheap `pure e` payload of a never-a-Lua-function type escapes BARE
/// to the caller's __mll_run — Integer (boxed bignum table) and ByteString
/// (a Lua string) are on that list (ty_never_lua_function), so the
/// terminal below returns the parameter, not an __mll_pure box.
#[test]
fn pure_bytestring_param_escapes_bare() {
    let source = r#"
passBack :: ByteString -> IO ByteString
passBack b = do
    let n = bsLength b
    if n > 0 then pure b else pure b

main :: IO ()
main = do
    b <- passBack (bsReplicate 3 65)
    print (bsLength b)
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    assert!(
        !lua.contains("__mll_pure(b)"),
        "a cheap ByteString pure payload must escape bare: {lua}"
    );
}

/// Constructor-level DCE: a `data` definition none of whose constructors is
/// constructed (`Con`) or matched (pattern) by live code contributes NOTHING
/// to the emitted Lua. Checked two ways: (1) adding a dead user `data`
/// declaration (with derived instances) to a program leaves the output
/// byte-identical; (2) a minimal program carries no constructor slots for the
/// four Prelude datatypes (ExitValue, Any, Either, Ordering — 12 slots before
/// this pass), pinned as a bound on `__mll_fn` slot assignments.
#[test]
fn constructor_dce_unused_data_adds_nothing() {
    let base = "double :: Int -> Int\n\
                double x = x * 2\n\
                main :: IO ()\n\
                main = print (double 21)\n";
    let with_dead = format!(
        "data Unused = UnusedA | UnusedB Int deriving (Show, Eq)\n{}",
        base
    );
    let dir = Path::new("tests/cases");
    let a = compile(base, dir, &[]).expect("base must compile").lua_code;
    let b = compile(&with_dead, dir, &[])
        .expect("dead-data program must compile")
        .lua_code;
    assert!(
        a == b,
        "an unused data declaration must add nothing to the emitted Lua"
    );

    // No Prelude constructor slots in a minimal program: every `__mll_fn[N] =`
    // assignment left is a live function. Before constructor-DCE this program
    // carried 12 extra constructor slots; a loose bound keeps the test stable
    // against unrelated emission drift while still catching that regression.
    let slot_assigns = a.matches("__mll_fn[").count();
    assert!(
        slot_assigns < 10,
        "expected no Prelude constructor slots in a minimal program, \
         found {} __mll_fn references:\n{}",
        slot_assigns,
        a
    );
}

/// Constructor-level DCE must drop only the constructor FUNCTIONS, never the
/// type's metadata: a value of a dropped type can still flow through live
/// code without being constructed or matched there. The canonical case is a
/// LuaDict record built by the Lua host and read only through accessors — its
/// keyed table layout comes from the registered definition, so the accessor
/// must still emit `.port` (not a positional index) and the FFI decoder must
/// keep the declared field types, while the constructor itself is not
/// emitted.
#[test]
fn constructor_dce_keeps_metadata_for_flow_through_types() {
    let source = "data Config = Config\n\
                  \x20 { port :: Int\n\
                  \x20 , host :: String\n\
                  \x20 } deriving (LuaDict)\n\
                  export readPort :: Config -> Int\n\
                  readPort c = port c\n";
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("flow-through program must compile")
        .lua_code;
    // The accessor keeps the LuaDict keyed layout (metadata survived)...
    assert!(
        lua.contains(").port"),
        "accessor must read the LuaDict field by key, not position:\n{}",
        lua
    );
    // ...the FFI decoder still knows the declared record shape...
    assert!(
        lua.contains("t=\"Config\""),
        "FFI decode descriptor must keep the record's field metadata:\n{}",
        lua
    );
    // ...but the never-constructed constructor function is not emitted.
    assert!(
        !lua.contains("port = _p0"),
        "the unconstructed Config constructor must not be emitted:\n{}",
        lua
    );
}

/// Cheap-eagerness must stay sound but not over-tighten: a let binding whose
/// RHS only reads provably-WHNF variables (a literal-bound sibling, a
/// demand-analysis-strict parameter) is still assigned strictly — no thunk —
/// while a binding reading a non-WHNF variable must be thunked. This is the
/// eagerness half of the lazy_cheap_bindings.mll regression, which runtime
/// behaviour alone cannot observe.
#[test]
fn cheap_eagerness_whnf_bindings_stay_strict() {
    let source = r#"
f :: Int -> Int
f x = let n = 5
          m = n + x
      in m + n

g :: Bool -> Int
g x = let y = error "boom"
          z = y + 1
      in if x then z else 0

main :: IO ()
main = putStrLn (show (f 10 + g False))
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    // n is bound to a literal: strict assignment, no thunk.
    assert!(lua.contains("n = 5"), "literal binding should be assigned strictly");
    assert!(!lua.contains("n = __thunk"), "literal binding must not be thunked");
    // m reads only provably-WHNF vars (n is literal-bound and already
    // assigned; x is a demand-strict parameter forced at entry): strict.
    assert!(
        !lua.contains("m = __thunk"),
        "binding over provably-WHNF variables must stay eagerly assigned:\n{}",
        lua
    );
    // z reads y, a thunked bottom: z itself must be thunked, and y must
    // never be forced outside a thunk body at binding time.
    assert!(
        lua.contains("z = __thunk"),
        "binding over a non-WHNF variable must be thunked:\n{}",
        lua
    );
}

/// The Prelude builtin `exit :: ExitValue -> IO ()` must resolve to the
/// runtime helper `exit_`, which unwraps the ExitValue ADT (Normal = {1},
/// Err code = {2, code}) and calls os.exit. Before the fix the emitted Lua
/// referenced an undefined global `exit` and crashed at runtime.
///
/// This test only inspects the emitted Lua — never execute `exit`
/// in-process: os.exit would terminate the whole cargo test harness.
/// Actual exit-code behaviour is covered by the subprocess tests in
/// mll/tests/exit_builtin.rs.
#[test]
fn exit_builtin_resolves_to_runtime_helper() {
    for src in [
        "main :: IO ()\nmain = exit Normal\n",
        "main :: IO ()\nmain = exit (Err 3)\n",
    ] {
        let lua = compile(src, Path::new("tests/cases"), &[])
            .expect("compile should succeed")
            .lua_code;
        assert!(
            !lua.contains("__force(exit)"),
            "surface `exit` was left as an undefined Lua global:\n{}",
            lua
        );
        assert!(
            lua.contains("exit_("),
            "call site should resolve to the exit_ runtime helper:\n{}",
            lua
        );
        assert!(
            lua.contains("local function exit_"),
            "exit_ runtime helper chunk missing from emitted prelude:\n{}",
            lua
        );
        assert!(
            lua.contains("os.exit"),
            "exit_ helper should call os.exit:\n{}",
            lua
        );
    }
}

/// Every compiled module carries compiler provenance: __MLLC_VERSION (the mllc
/// crate version) and __MLLC_COMMIT (the full git commit it was built from),
/// emitted as top-level locals and surfaced through the export table.
///
/// The workspace shares a single version line, so this test crate's
/// CARGO_PKG_VERSION equals mllc's — asserting equality also guards against a
/// version desync between crates.
#[test]
fn compiled_module_carries_mllc_provenance() {
    // A plain program (has main, no exports): stamps present as locals.
    let prog = "main :: IO ()\nmain = putStrLn \"hi\"\n";
    let lua = compile(prog, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;

    let ver_line = format!("local __MLLC_VERSION = \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(
        lua.contains(&ver_line),
        "module must stamp the mllc crate version (expected `{}`):\n{}",
        ver_line,
        lua
    );
    // __MLLC_COMMIT is a full 40-char git hash, or "unknown" for a non-git
    // (e.g. crates.io tarball) build.
    let commit = lua
        .lines()
        .find_map(|l| l.strip_prefix("local __MLLC_COMMIT = \"").and_then(|r| r.strip_suffix('"')))
        .expect("module must stamp a __MLLC_COMMIT local");
    assert!(
        commit == "unknown"
            || (commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit())),
        "__MLLC_COMMIT must be a full 40-hex commit or \"unknown\", got `{commit}`"
    );

    // An exporting module surfaces the stamps as module properties, so a Lua
    // host can read them from the required table.
    let module = "export answer :: Int\nanswer :: Int\nanswer = 42\n";
    let lua = compile(module, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    assert!(
        lua.contains("__MLLC_VERSION = __MLLC_VERSION"),
        "export table must expose __MLLC_VERSION as a module property:\n{}",
        lua
    );
    assert!(
        lua.contains("__MLLC_COMMIT = __MLLC_COMMIT"),
        "export table must expose __MLLC_COMMIT as a module property:\n{}",
        lua
    );
}

/// A `module M (…) where` header export list scopes .mll import visibility
/// only — by design it does not export anything to the Lua host (that is the
/// `export` keyword's job; see SPEC.md "Module and import syntax"). A module
/// whose only "exports" are header-listed therefore has no host surface when
/// compiled standalone: no `main`, nothing in the return table, and dead-code
/// elimination removes every definition. That used to produce an empty Lua
/// A partial application's thunked captured argument is hoisted OUT of
/// the closure (Q76): left inline, every invocation of the closure
/// allocated a fresh thunk — each memoizing separately, so the captured
/// computation re-ran per call where GHC shares one thunk. Eager captured
/// arguments stay inline (hoisting would evaluate them at build time).
#[test]
fn partial_application_hoists_thunked_captures() {
    let src = "pick2 :: Int -> Int -> Int\n\
               pick2 x y = if y > 0 then x + y else y\n\n\
               heavy :: Int -> Int\n\
               heavy 0 = 0\n\
               heavy n = n + heavy (n - 1)\n\n\
               main :: IO ()\n\
               main = print (sum (map (pick2 (heavy 3)) [1, 2, 0 - 1]))\n";
    let result = compile(src, Path::new("tests/cases"), &[])
        .expect("compiles");
    assert!(
        result.lua_code.contains("local _pc0 = __thunk("),
        "thunked capture must be bound once, outside the closure:\n{}",
        result.lua_code
    );
    assert!(
        result.lua_code.contains("(_pc0, _pa0)"),
        "the closure must reference the shared thunk:\n{}",
        result.lua_code
    );
}

/// Structured demand rows respect shadowing (F2): a case binder named like
/// a strict where-local masks the local's row, so a binding passed through
/// the BINDER stays a thunk (the real callee — here `pick` — never demands
/// it; eager evaluation raised a bottom GHC never touches). The unshadowed
/// twin keeps the strict assignment — the fix must not over-mask genuine
/// demand.
#[test]
fn shadowed_where_local_row_keeps_binding_lazy() {
    let shadowed = "f :: Int -> Int -> Int\n\
                    f x y = case pick of\n\
                    \x20   go -> go bad\n\
                    \x20 where\n\
                    \x20   go n = n + 1\n\
                    \x20   bad = y + 1\n\
                    \x20   pick = \\_ -> 0\n\
                    \x20   unused = go x\n\n\
                    main :: IO ()\n\
                    main = print (f 7 (error \"never demanded\"))\n";
    let result = compile(shadowed, Path::new("tests/cases"), &[])
        .expect("compiles");
    assert!(
        result.lua_code.contains("bad = __thunk("),
        "a binding passed to the shadowing case binder must stay lazy:\n{}",
        result.lua_code
    );

    let genuine = "f :: Int -> Int -> Int\n\
                   f x y = case pick of\n\
                   \x20   h -> go bad\n\
                   \x20 where\n\
                   \x20   go n = n + 1\n\
                   \x20   bad = y + 1\n\
                   \x20   pick = \\_ -> 0\n\n\
                   main :: IO ()\n\
                   main = print (f 7 (error \"genuinely demanded\"))\n";
    let result = compile(genuine, Path::new("tests/cases"), &[])
        .expect("compiles");
    assert!(
        result.lua_code.contains("bad = y + 1"),
        "genuine demand through the surviving row must stay strict \
         (y is entry-forced via f's row, so the binding reads it direct):\n{}",
        result.lua_code
    );
}

/// An import alias that is also a data constructor: the constructor wins
/// (alias_ctor_collision.mll pins the program meaning), and the compiler
/// must SAY that qualified references through the alias will not resolve
/// (Q67) — silently dropping the alias would leave `M.size` failing with
/// an unexplained unbound-variable error.
#[test]
fn alias_constructor_collision_warns() {
    let src = "import qualified AliasCtor as M\n\n\
               data Mode = M | N\n\n\
               tag :: Mode -> Int\n\
               tag M = 1\n\
               tag N = 2\n\n\
               main :: IO ()\n\
               main = print (tag M)\n";
    let result = compile(src, Path::new("tests/cases"), &[])
        .expect("the constructor meaning must compile");
    assert_eq!(result.warnings.len(), 1, "exactly one collision warning");
    let rendered = format!("{}", result.warnings[0]);
    assert!(
        rendered.contains("import alias 'M' is also a data constructor"),
        "warning must name the collision:\n{}",
        rendered
    );
    assert!(
        rendered.contains("rename the alias"),
        "warning must offer the fix:\n{}",
        rendered
    );

    // A non-colliding alias stays fully functional and warns nothing.
    let clean = "import qualified AliasCtor as A\n\n\
                 main :: IO ()\n\
                 main = print A.size\n";
    let result = compile(clean, Path::new("tests/cases"), &[])
        .expect("non-colliding alias compiles");
    assert!(
        result.warnings.is_empty(),
        "no warning without a collision: {:?}",
        result.warnings.iter().map(|w| format!("{}", w)).collect::<Vec<_>>()
    );
    assert!(result.lua_code.contains("99"), "qualified use resolves");
}

/// shell *silently*; the compiler must now say so via `CompileResult.warnings`.
#[test]
fn header_only_root_module_warns_instead_of_silent_empty_output() {
    let src = "module C (addup) where\n\n\
               addup :: Int -> Int -> Int\n\
               addup a b = a + b\n";
    let result = compile(src, Path::new("tests/cases"), &[])
        .expect("a header-form library must still compile");

    // Documented semantics: the header list creates no host surface.
    assert!(!result.has_main);
    assert!(
        result.exports.is_empty(),
        "header export lists must not populate the Lua exports: {:?}",
        result.exports
    );
    assert!(
        !result.lua_code.contains("-- Exports"),
        "no export table may be emitted for a header-only module:\n{}",
        result.lua_code
    );
    assert!(
        !result.lua_code.contains("addup"),
        "with no main/export roots, the definition is dead code:\n{}",
        result.lua_code
    );

    // ... but not silently: the result must carry a warning that names the
    // mixup and the fix.
    assert_eq!(
        result.warnings.len(),
        1,
        "exactly one no-host-surface warning expected"
    );
    let rendered = format!("{}", result.warnings[0]);
    assert!(
        rendered.contains("no runnable or callable code"),
        "warning must state the consequence:\n{}",
        rendered
    );
    assert!(
        rendered.contains("module … (addup) where"),
        "warning must point at the header export list:\n{}",
        rendered
    );
    assert!(
        rendered.contains("export addup"),
        "warning must show the `export` keyword fix:\n{}",
        rendered
    );
}

/// The generic form of the same warning: a bare library (no module header at
/// all) compiled as the root also has nothing to run or call.
/// Cross-binding constant folding (fold.rs fixpoint): a literal CAF
/// propagates into its use sites, a saturated call to a total-arithmetic
/// function on literal arguments beta-reduces, and the folded binding is
/// itself a literal CAF for ITS users — so `ghi` reaches `main` as the
/// literal result and abc/def/ghi are dead code. Distinctive literals
/// (absent from the runtime prelude) mark the fold: the result value must
/// appear, the ingredients must not.
#[test]
fn cross_binding_constants_fold_to_literals() {
    let source = r#"
abc :: Int
abc = 41001

def :: Int -> Int
def x = x + 1002

ghi :: Int
ghi = abc + def 40000

main :: IO ()
main = print ghi
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    assert!(lua.contains("82003"), "ghi must fold to the literal 82003: {lua}");
    for ingredient in ["41001", "1002", "40000"] {
        assert!(
            !lua.contains(ingredient),
            "folded ingredient `{ingredient}` must not survive to emission: {lua}"
        );
    }
}

/// A fully constant program collapses at compile time: the fold splices
/// the wrapper chain (`print` → `putStrLn (show x)` → the FFI call) at
/// the literal call site and folds `show` of the Int literal to its
/// string, so `main` performs the host `print` on the shown constant
/// directly.  Nothing of the chain survives: no `show_Int` call (and
/// hence no Burger–Dybvig double formatter in the on-demand prelude),
/// no FFI wrapper slot (the spliced SpecCall must not retain its dead
/// origin through DCE), no boxed argument marshalling.
#[test]
fn constant_program_collapses_to_direct_print() {
    let source = r#"
abc :: Int
abc = 17

def :: Int -> Int
def x = x + 1

ghi :: Int
ghi = abc + def 5

main :: IO ()
main = print ghi
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    assert!(
        lua.contains(r#"print("23")"#),
        "main must perform the host print on the folded, shown constant: {lua}"
    );
    for leftover in ["show_Int", "__mll_show_double", "_ffi0"] {
        assert!(
            !lua.contains(leftover),
            "the collapsed chain must leave no `{leftover}` behind: {lua}"
        );
    }
    // Exactly one emitted function: main. The print@Int specialization and
    // the putStrLn FFI wrapper are dead after the splice.
    assert_eq!(
        lua.matches("__mll_fn[").count(),
        2, // main's definition + the entry-point call
        "only the main slot may survive (one definition, one entry call): {lua}"
    );
}

/// The boundaries of the cross-binding folds: anything that could change
/// runtime-observable behavior is declined. A trapping literal divisor
/// stays a runtime `__mll_div` (raising only when demanded), and a local
/// binder masks a top-level candidate of the same name (the local runs).
#[test]
fn cross_binding_folding_declines_traps_and_shadowed_names() {
    let source = r#"
overZero :: Int -> Int
overZero x = x `div` 0

kept :: Int
kept = overZero 90071

shadowedFn :: Int
shadowedFn = bump 45013
  where bump y = y * 2

bump :: Int -> Int
bump y = y + 1

main :: IO ()
main = do
    print shadowedFn
    print (if shadowedFn > 0 then 0 else kept)
"#;
    let lua = compile(source, Path::new("tests/cases"), &[])
        .expect("compile should succeed")
        .lua_code;
    // The trap is left for the runtime: the division core survives and the
    // call was not replaced by any literal.
    assert!(lua.contains("__mll_div("), "literal zero divisor must stay on __mll_div: {lua}");
    assert!(lua.contains("90071"), "the declined call's argument must survive: {lua}");
    // The where-bound `bump` masks the top-level candidate: the call is
    // declined (the LOCAL bump computes at runtime, so the argument
    // survives) and the top-level body's result (45013 + 1 = 45014) is
    // never folded in through the local binder.
    assert!(lua.contains("45013"), "the masked call's argument must survive: {lua}");
    assert!(!lua.contains("45014"), "top-level bump must not fold through the local binder: {lua}");
}

#[test]
fn bare_library_root_warns_with_generic_guidance() {
    let src = "addup :: Int -> Int -> Int\naddup a b = a + b\n";
    let result = compile(src, Path::new("tests/cases"), &[])
        .expect("compile should succeed");
    assert_eq!(result.warnings.len(), 1);
    let rendered = format!("{}", result.warnings[0]);
    assert!(rendered.contains("no runnable or callable code"), "{}", rendered);
    assert!(rendered.contains("main :: IO ()"), "{}", rendered);
    assert!(rendered.contains("export"), "{}", rendered);
}

/// Modules with a host surface must stay warning-free — a program with
/// `main`, an `export`-keyword module, and (critically) a root program that
/// *imports* a header-form library: the library's header exports must neither
/// warn nor leak into the importer's return table.
#[test]
fn modules_with_a_host_surface_do_not_warn() {
    // Program root.
    let prog = "main :: IO ()\nmain = putStrLn \"hi\"\n";
    let result = compile(prog, Path::new("tests/cases"), &[])
        .expect("compile should succeed");
    assert!(result.warnings.is_empty(), "program with main must not warn");

    // `export`-keyword module root.
    let module = "export answer :: Int\nanswer :: Int\nanswer = 42\n";
    let result = compile(module, Path::new("tests/cases"), &[])
        .expect("compile should succeed");
    assert!(result.warnings.is_empty(), "exporting module must not warn");
    assert_eq!(result.exports, vec!["answer".to_string()]);

    // Program root importing a header-form library (ExportHelper's header
    // lists publicFn and PublicType(..)).
    let importer = "import ExportHelper\n\n\
                    main :: IO ()\n\
                    main = putStrLn (show (publicFn 4))\n";
    let result = compile(importer, Path::new("tests/cases"), &[])
        .expect("compile should succeed");
    assert!(
        result.warnings.is_empty(),
        "importing a header-form library must not warn"
    );
    assert!(
        result.exports.is_empty() && !result.lua_code.contains("-- Exports"),
        "an imported library's header exports must not leak into the \
         importer's return table:\n{}",
        result.lua_code
    );
}

#[test]
fn prelude_is_emitted_on_demand() {
    // A trivial program must not carry runtime helpers it never references.
    let trivial = compile("main :: IO ()\nmain = putStrLn \"hi\"\n", Path::new("."), &[])
        .expect("trivial should compile")
        .lua_code;
    assert!(!trivial.contains("show_HashMap"), "unused hashmap show must be shaken out");
    assert!(!trivial.contains("__mll_st_new"), "unused ST-array runtime must be shaken out");
    assert!(!trivial.contains("hashmap_insert"), "unused hashmap runtime must be shaken out");

    // But a program that uses a feature must still carry its runtime, or it
    // would break at runtime — reachability, not blanket removal.
    let uses_list_show = compile("main :: IO ()\nmain = print [1, 2, 3 :: Int]\n", Path::new("."), &[])
        .expect("list-show program should compile")
        .lua_code;
    assert!(uses_list_show.contains("__mll_show_list"), "list show must be present when used");

    // The prelude tracks usage: the trivial program's output is smaller than
    // the one that pulls in list show — not the whole runtime in both.
    assert!(trivial.len() < uses_list_show.len(),
        "on-demand prelude should track usage, not emit the whole runtime \
         (trivial {} bytes vs list-show {} bytes)", trivial.len(), uses_list_show.len());
}

#[test]
fn dead_code_is_eliminated() {
    let fn_count = |src: &str| -> usize {
        compile(src, Path::new("."), &[])
            .expect("should compile")
            .lua_code
            .matches("= function").count()
    };
    // A trivial program must not carry the unused auto-prelude.
    let trivial = fn_count("main :: IO ()\nmain = putStrLn \"hi\"\n");
    let prelude_heavy = fn_count(
        "main :: IO ()\nmain = print (foldr (+) 0 (map (\\x -> x * 2) (filter (\\x -> x > 0) [1, 2, 3, 4 :: Int])))\n",
    );
    assert!(trivial < prelude_heavy,
        "trivial ({trivial} fns) should emit fewer functions than prelude-heavy ({prelude_heavy} fns)");
    assert!(trivial < 25, "trivial program should be tiny after DCE, got {trivial} fns");

    // Exports are roots: an exported function survives DCE even when `main`
    // never calls it (it is reachable only from outside).
    let (_lua, module) = compile_ffi_module(
        "export twice :: Int -> Int\ntwice x = x + x\nmain :: IO ()\nmain = pure ()\n",
    );
    let twice: mlua::Function = module.get("twice").unwrap();
    let r: i64 = twice.call(21).unwrap();
    assert_eq!(r, 42, "exported function must survive DCE and run");
}

// --- Source embedding (--embed-source) and recompilation (--recompile) ---
//
// Compiling with `CompileOptions { embed_source: Some(mode) }` must carry the
// original .mll source inside the emitted Lua so `embed::extract_source` can
// recover it byte-exactly and recompile it without the .mll file. The fixture
// is deliberately hostile: long-bracket closers at several levels (forcing
// the embedder to pick a non-colliding bracket level), fake marker lines,
// and escaped quotes — none of which may terminate the block early or break
// the emitted Lua.

const EMBED_FIXTURE: &str = r#"-- bracket bombs in a comment: ]] ]=] ]==] --[[
-- MLL-EMBEDDED-SOURCE-END ]=]
greeting :: String
greeting = "brackets: ]] ]=] ]==] ]===] and --[==[ and \"quotes\""

fakeMarker :: String
fakeMarker = "local __SOURCE_CODE = [=["

main :: IO ()
main = do
  putStrLn greeting
  putStrLn fakeMarker
"#;

fn compile_embedded(source: &str, mode: mllc::EmbedMode) -> String {
    let opts = mllc::CompileOptions { embed_source: Some(mode), ..Default::default() };
    with_compiler_stack(|| mllc::compile_with_options(source, Path::new("."), &[], &opts))
        .expect("embedding compile should succeed")
        .lua_code
}

#[test]
fn embed_comments_round_trip() {
    let plain = compile(EMBED_FIXTURE, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    // Without embedding, no marker or source variable may leak into the output.
    assert!(!plain.contains("MLL-EMBEDDED-SOURCE-BEGIN"),
        "plain compile must not carry an embedded-source block");

    let embedded = compile_embedded(EMBED_FIXTURE, mllc::EmbedMode::Comments);

    // The embedded file is still loadable, runnable Lua.
    let lua = mlua::Lua::new();
    lua.load(&embedded).set_name("embed_comments").exec()
        .expect("comment-embedded Lua should still run");

    // Extraction recovers the source byte-exactly, with the right mode.
    let (extracted, mode) = mllc::embed::extract_source(&embedded)
        .expect("embedded source should be found");
    assert_eq!(mode, mllc::EmbedMode::Comments);
    assert_eq!(extracted, EMBED_FIXTURE, "extraction must be byte-exact");

    // Recompiling the extracted source matches a direct compile exactly...
    let recompiled = compile(&extracted, Path::new("."), &[])
        .expect("extracted source should recompile")
        .lua_code;
    assert_eq!(recompiled, plain, "recompile must equal a direct compile");

    // ...and re-embedding reproduces the embedded file byte-for-byte, so an
    // in-place `--recompile` of an unchanged file is a fixpoint.
    let reembedded = compile_embedded(&extracted, mllc::EmbedMode::Comments);
    assert_eq!(reembedded, embedded, "re-embedding must be a fixpoint");
}

#[test]
fn embed_var_round_trip() {
    let plain = compile(EMBED_FIXTURE, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    // The fixture's fake-marker string literal appears in the plain output
    // (as a generated Lua string constant), so a bare substring check would
    // misfire — what matters is that it doesn't extract as a source block.
    assert!(mllc::embed::extract_source(&plain).is_err(),
        "plain compile must not carry an extractable source block");

    let embedded = compile_embedded(EMBED_FIXTURE, mllc::EmbedMode::Var);

    // Loaded as a module (chunk argument set, so main is skipped), the
    // exports table carries the exact source under __SOURCE_CODE.
    let lua = mlua::Lua::new();
    let exports: mlua::Table = lua.load(&embedded).set_name("embed_var")
        .call("embed_var")
        .expect("var-embedded Lua should load as a module");
    let runtime_source: String = exports.get("__SOURCE_CODE")
        .expect("module must export __SOURCE_CODE");
    assert_eq!(runtime_source, EMBED_FIXTURE,
        "__SOURCE_CODE at runtime must equal the original source");

    // It also still runs as a program (main not skipped).
    let lua = mlua::Lua::new();
    lua.load(&embedded).set_name("embed_var").exec()
        .expect("var-embedded Lua should still run as a program");

    // Textual extraction round-trips byte-exactly and recompiles identically.
    let (extracted, mode) = mllc::embed::extract_source(&embedded)
        .expect("embedded source should be found");
    assert_eq!(mode, mllc::EmbedMode::Var);
    assert_eq!(extracted, EMBED_FIXTURE, "extraction must be byte-exact");
    let recompiled = compile(&extracted, Path::new("."), &[])
        .expect("extracted source should recompile")
        .lua_code;
    assert_eq!(recompiled, plain, "recompile must equal a direct compile");
    let reembedded = compile_embedded(&extracted, mllc::EmbedMode::Var);
    assert_eq!(reembedded, embedded, "re-embedding must be a fixpoint");
}

#[test]
fn embed_var_merges_with_existing_exports() {
    // A module with real `export`s: __SOURCE_CODE must join the exports table
    // without displacing them.
    let source = r#"
export double :: Int -> Int
double n = n * 2

main :: IO ()
main = putStrLn (show (double 4))
"#;
    let embedded = compile_embedded(source, mllc::EmbedMode::Var);
    let lua = mlua::Lua::new();
    let exports: mlua::Table = lua.load(&embedded).set_name("embed_exports")
        .call("embed_exports")
        .expect("should load as a module");
    let runtime_source: String = exports.get("__SOURCE_CODE")
        .expect("module must export __SOURCE_CODE");
    assert_eq!(runtime_source, source);
    let double: mlua::Function = exports.get("double")
        .expect("real exports must survive alongside __SOURCE_CODE");
    let result: i64 = double.call(21).expect("exported fn should be callable");
    assert_eq!(result, 42);
}

#[test]
fn embed_source_without_trailing_newline() {
    // The framing newlines belong to the block, not the source: a source with
    // no final newline must come back without one.
    let source = "main :: IO ()\nmain = putStrLn \"no trailing newline\"";
    for mode in [mllc::EmbedMode::Comments, mllc::EmbedMode::Var] {
        let embedded = compile_embedded(source, mode);
        let lua = mlua::Lua::new();
        lua.load(&embedded).set_name("embed_no_nl").exec()
            .expect("should run");
        let (extracted, _) = mllc::embed::extract_source(&embedded)
            .expect("embedded source should be found");
        assert_eq!(extracted, source, "byte-exact round trip ({:?})", mode);
    }
}

#[test]
fn extract_from_plain_lua_rejected() {
    let plain = compile(EMBED_FIXTURE, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    match mllc::embed::extract_source(&plain) {
        Err(e) => assert!(e.contains("no embedded MLL source"),
            "expected a clear no-embedded-source message, got: {}", e),
        Ok(_) => panic!("extraction from a plain compile must fail"),
    }
}

// ---------------------------------------------------------------------------
// Regression tests: string escaping is canonical (one escaper for expression
// literals, pattern literals, and LuaDict table keys — see codegen.rs
// `lua_quoted_string`).
// ---------------------------------------------------------------------------

/// A string literal in a PATTERN used to be emitted with no escaping at all,
/// so `f "a\"b" = 1` produced `if _arg0 == "a"b" then` — Lua that failed to
/// load. The pattern path must go through the same canonical escaper as
/// expression literals: the program must compile, load, AND match correctly.
#[test]
fn string_pattern_literal_with_quote_and_newline_is_escaped() {
    let source = r#"
f :: String -> Int
f "a\"b\nc" = 1
f _ = 0

main :: IO ()
main = if f "a\"b\nc" == 1 && f "x" == 0
         then putStrLn "ok"
         else error "string pattern with escapes matched incorrectly"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("a string pattern containing a quote and a newline must compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code)
        .exec()
        .expect("the emitted Lua must load and the pattern must match correctly");
}

/// A LuaDict `as`-renamed field key containing control characters used to be
/// emitted raw inside `["…"]` (only `"` and `\` were escaped), producing an
/// unfinished-string Lua syntax error. The key path must use the canonical
/// escaper too: construct, read the field back, and check the wire key.
#[test]
fn luadict_as_key_with_control_chars_is_escaped() {
    let source = r#"
data Rec = Rec { field1 as "a\nb\tc" :: Int } deriving (Show, LuaDict)

main :: IO ()
main = if field1 (Rec 5) == 5
         then putStrLn "ok"
         else error "field behind a control-character as-key read back wrong"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("an as-key containing \\n and \\t must compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code)
        .exec()
        .expect("the emitted Lua must load and the renamed field must round-trip");
}
