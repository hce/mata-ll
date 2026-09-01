//! G5: the strictness masks in demand.rs (`STRICT_BUILTINS`,
//! `RUNTIME_PRELUDE_STRICTNESS`, `PRIMITIVE_BINOP_METHODS`) are prose
//! mirrors of the emitted Lua runtime — each `true` claims the runtime body
//! forces that position on every path, which is what licenses eager
//! evaluation at call sites. Nothing machine-checked that claim until now:
//! this harness compiles a probe program that reaches every masked runtime
//! body, then calls each body with a bomb thunk per position and asserts
//! the recorded behavior — a strict position must raise the bomb, a lazy
//! one must not.
//!
//! Probe targets are resolved through codegen's own `sanitize_name`
//! (`bsLength` → `__mll_bs[1]`, `error` → `error_`), not a hand-kept copy.
//! Coverage is closed both ways: every mask entry must have a probe (a new
//! mask without one fails the test), and every probe must match its mask's
//! arity and strictness shape.
//!
//! Two deliberate mask/behavior gaps are encoded as `ForcedOnRun`: the
//! first-class ST closures (`__mll_ma_new`/`__mll_ma_write`) DO force the
//! initializer/stored value when the action runs, but their masks keep
//! those positions lazy because a built-but-never-run action must not
//! force anything (see the fused-mask note in codegen/action.rs). GHC
//! stores array elements lazily even on write, so the run-time force is a
//! recorded deviation (G8 candidate), asserted here so a change in either
//! direction is seen.

use super::*;

/// Expected behavior when position `i` receives a bomb thunk.
#[derive(Clone, Copy, PartialEq)]
enum Pos {
    /// Mask `true`: the body forces it on every path — the bomb must raise.
    Strict,
    /// Mask `false` and lazy on the sampled path — the bomb must NOT raise.
    Lazy,
    /// Mask `false` (deliberate under-claim, see module doc) but the
    /// sampled build+run forces it — the bomb must raise.
    ForcedOnRun,
}
use Pos::{ForcedOnRun, Lazy, Strict};

struct Probe {
    /// Source name: the mask key, and the input to `sanitize_name`.
    name: &'static str,
    /// One sample Lua expression per value argument.
    args: &'static [&'static str],
    positions: &'static [Pos],
    /// The call builds an ST action closure: run it to observe the forces.
    run_action: bool,
    /// The bomb-free control call itself raises (`error`'s entire purpose).
    control_raises: bool,
}

const fn probe(
    name: &'static str,
    args: &'static [&'static str],
    positions: &'static [Pos],
) -> Probe {
    Probe { name, args, positions, run_action: false, control_raises: false }
}

const fn st_probe(
    name: &'static str,
    args: &'static [&'static str],
    positions: &'static [Pos],
) -> Probe {
    Probe { name, args, positions, run_action: true, control_raises: false }
}

/// Sample fragments (defined in the probe preamble).
const IDF: &str = "function(x) return x end";
const TRUEF: &str = "function(x) return true end";
const ADD2: &str = "function(a, b) return 1 end";
const ILIST: &str = "__mll_cons(1, nil)";
const ILIST65: &str = "__mll_cons(65, nil)";
const SLIST: &str = "__mll_cons(\"a\", nil)";
const ARR: &str = "({7, 7})";
const REF: &str = "setmetatable({0}, __mll_ioref_mt)";
/// Boxed Integers, built by the runtime's own constructor (reachable —
/// `fromInteger_Integer` is itself probed). 3 divides nothing sampled, so
/// the division family's control calls stay off the zero-divisor error.
const INTG: &str = "fromInteger_Integer(5)";
const INTG2: &str = "fromInteger_Integer(3)";

/// Probes for `STRICT_BUILTINS` and `RUNTIME_PRELUDE_STRICTNESS`.
/// (`PRIMITIVE_BINOP_METHODS` probes are generated — all are 2-ary and
/// strict in both operands by definition.)
const PROBES: &[Probe] = &[
    // --- ByteString primitives ---
    probe("bsLength", &["\"hi\""], &[Strict]),
    probe("bsIndex", &["\"hi\"", "0"], &[Strict, Strict]),
    probe("bsSub", &["\"hi\"", "0", "1"], &[Strict, Strict, Strict]),
    probe("bsNull", &["\"hi\""], &[Strict]),
    probe("bsHead", &["\"hi\""], &[Strict]),
    probe("bsTail", &["\"hi\""], &[Strict]),
    probe("bsCons", &["65", "\"hi\""], &[Strict, Strict]),
    probe("bsSnoc", &["\"hi\"", "65"], &[Strict, Strict]),
    probe("bsConcat", &["\"a\"", "\"b\""], &[Strict, Strict]),
    probe("bsSingleton", &["65"], &[Strict]),
    probe("bsReplicate", &["2", "65"], &[Strict, Strict]),
    probe("bsGetU16LE", &["\"abcd\"", "0"], &[Strict, Strict]),
    probe("bsGetU32LE", &["\"abcd\"", "0"], &[Strict, Strict]),
    probe("bsGetI8", &["\"abcd\"", "0"], &[Strict, Strict]),
    probe("bsGetI16LE", &["\"abcd\"", "0"], &[Strict, Strict]),
    probe("bsPutI16LE", &["7"], &[Strict]),
    probe("bsToString", &["\"hi\""], &[Strict]),
    probe("bsFromString", &["\"hi\""], &[Strict]),
    probe("bsConcatList", &[SLIST], &[Strict]),
    probe("bsPack", &[ILIST65], &[Strict]),
    // --- first-class ST array closures (built, then run) ---
    st_probe("newSTArray", &["1", "0"], &[Strict, ForcedOnRun]),
    st_probe("readSTArray", &[ARR, "0"], &[Strict, Strict]),
    st_probe("writeSTArray", &[ARR, "0", "0"], &[Strict, Strict, ForcedOnRun]),
    st_probe("modifySTArray", &[ARR, "0", IDF], &[Strict, Strict, Strict]),
    st_probe("stArrayLength", &[ARR], &[Strict]),
    st_probe("newSTArrayFromList", &[ILIST], &[Strict]),
    st_probe("stArrayToList", &[ARR], &[Strict]),
    // --- first-class IORef closures (built, then run) ---
    // The value positions are Lazy on EVERY path — GHC parity (writeIORef
    // doesn't force the value, modifyIORef stores the suspension), unlike
    // the ST array ops' ForcedOnRun deviation. modifyIORef' calls f on the
    // run and forces its result, hence Strict in f.
    st_probe("newIORef", &["0"], &[Lazy]),
    st_probe("readIORef", &[REF], &[Strict]),
    st_probe("writeIORef", &[REF, "0"], &[Strict, Lazy]),
    st_probe("modifyIORef", &[REF, IDF], &[Strict, Lazy]),
    st_probe("modifyIORef'", &[REF, IDF], &[Strict, Strict]),
    // --- runtime-implemented prelude functions ---
    probe("show", &["1"], &[Strict]),
    probe("show_Int", &["1"], &[Strict]),
    probe("show_Number", &["1.5"], &[Strict]),
    probe("show_String", &["\"s\""], &[Strict]),
    probe("show_Bool", &["true"], &[Strict]),
    probe("show_List_", &[ILIST], &[Strict]),
    probe("show_Maybe", &["nil"], &[Strict]),
    probe("show_ByteString", &["\"s\""], &[Strict]),
    probe("show_HashMap", &["({})"], &[Strict]),
    probe("not", &["true"], &[Strict]),
    Probe {
        name: "error",
        args: &["\"msg\""],
        positions: &[Strict],
        run_action: false,
        control_raises: true,
    },
    probe("head", &[ILIST], &[Strict]),
    probe("tail", &[ILIST], &[Strict]),
    probe("map", &[IDF, ILIST], &[Strict, Strict]),
    probe("filter", &[TRUEF, ILIST], &[Strict, Strict]),
    // take is LAZY in the list when n <= 0 — `take 0 undefined` is `[]`
    // (the mask's one deliberate laziness hole; GHC-visible).
    probe("take", &["0", ILIST], &[Strict, Lazy]),
    probe("drop", &["1", ILIST], &[Strict, Strict]),
    probe("zipWith", &[ADD2, ILIST, ILIST], &[Strict, Strict, Strict]),
    // --- the boxed-Integer core (strict in every position — a limb walk
    // cannot go through a thunk; see the demand.rs block comment) ---
    probe("fromInteger_Integer", &["5"], &[Strict]),
    probe("toInteger_Integer", &[INTG], &[Strict]),
    probe("toInteger_Int", &["5"], &[Strict]),
    probe("negate_Integer", &[INTG], &[Strict]),
    probe("abs_Integer", &[INTG], &[Strict]),
    probe("signum_Integer", &[INTG], &[Strict]),
    probe("show_Integer", &[INTG], &[Strict]),
    probe("add_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("sub_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("mul_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("quot_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("rem_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("quotRem_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("div_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("mod_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("divMod_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("eq_Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("ord_lt__Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("ord_gt__Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("ord_le__Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("ord_ge__Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("ord_max__Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("ord_min__Integer", &[INTG, INTG2], &[Strict, Strict]),
    probe("ord_compare__Integer", &[INTG, INTG2], &[Strict, Strict]),
];

/// `PRIMITIVE_BINOP_METHODS` entries with no reachable named fallback: the
/// named `semigroup_String` local is referenced only from dictionary
/// shapes, and String is Semigroup's ONLY instance — no program can need a
/// Semigroup dictionary at a still-polymorphic type, so first-class `<>`
/// resolves to the Prelude's own definition instead. Its inline path
/// (`a .. b` over forced operands) is strict by Lua semantics.
const PRIMITIVE_EXCLUSIONS: &[&str] = &["semigroup_String"];

/// Sample operand pair for a primitive method, from its type-name suffix.
fn primitive_samples(name: &str) -> (&'static str, &'static str) {
    let suffix = name.rsplit('_').next().unwrap_or("");
    match suffix {
        "Int" => ("1", "2"),
        "Number" => ("1.5", "2.5"),
        "String" | "ByteString" => ("\"a\"", "\"b\""),
        "Bool" => ("true", "false"),
        other => panic!("no sample operands for primitive type '{other}' ({name})"),
    }
}

/// The probe program: reaches every masked runtime body so the on-demand
/// prelude emits it. `once` keeps the ST intrinsics first-class (the fused
/// `__mll_st_*` twins replace them in run-once positions); the `deep`/
/// `deepM` dictionary shapes reach the type-erased `show_List_`/
/// `show_Maybe` shims; the op lists keep the primitive method fallbacks
/// first-class (inline uses compile to bare Lua operators instead).
const PROBE_PROGRAM: &str = r#"
module Main where

once :: ST s a -> ST s a
once a = a

onceIO :: IO a -> IO a
onceIO a = a

run2 :: [a -> a -> b] -> a -> a -> [b]
run2 fs x y = map (\f -> f x y) fs

opsI :: [Int -> Int -> Bool]
opsI = [(==), (<), (>), (<=), (>=)]

opsN :: [Number -> Number -> Bool]
opsN = [(==), (<), (>), (<=), (>=)]

opsS :: [String -> String -> Bool]
opsS = [(==), (<), (>), (<=), (>=)]

opsB :: [ByteString -> ByteString -> Bool]
opsB = [(==), (<), (>), (<=), (>=)]

opsBool :: [Bool -> Bool -> Bool]
opsBool = [(==)]

mmI :: [Int -> Int -> Int]
mmI = [max, min]

mmN :: [Number -> Number -> Number]
mmN = [max, min]

mmS :: [String -> String -> String]
mmS = [max, min]

mmB :: [ByteString -> ByteString -> ByteString]
mmB = [max, min]

deep :: Show a => Int -> a -> String
deep 0 x = show x
deep n x = deep (n - 1) [x]

deepM :: Show a => Int -> a -> String
deepM 0 x = show x
deepM n x = deepM (n - 1) (Just x)

main :: IO ()
main = do
  let b = bsFromString "hello"
  print (bsLength b)
  print (runST (do
    arr <- once (newSTArray 2 (0 :: Int))
    once (writeSTArray arr 0 5)
    once (modifySTArray arr 0 (+ 1))
    x <- once (readSTArray arr 0)
    n <- once (stArrayLength arr)
    arr2 <- once (newSTArrayFromList [1, 2, 3 :: Int])
    l <- once (stArrayToList arr2)
    pure (x + n + length l)))
  ref <- onceIO (newIORef (0 :: Int))
  onceIO (writeIORef ref 1)
  onceIO (modifyIORef ref (+ 1))
  onceIO (modifyIORef' ref (+ 1))
  rv <- onceIO (readIORef ref)
  print rv
  print (1 :: Int)
  print (1.5 :: Number)
  print "s"
  print True
  print [4 :: Int]
  print (Just (3 :: Int))
  print b
  print (hmInsert (1 :: Int) (2 :: Int) hmEmpty)
  print (not False)
  if bsNull b then error "empty" else pure ()
  print (head [1 :: Int, 2])
  print (tail [1 :: Int, 2])
  print (map (+ 1) [1 :: Int])
  print (filter even [1 :: Int, 2])
  print (take 1 [1 :: Int, 2])
  print (drop 1 [1 :: Int, 2])
  print (zipWith (+) [1 :: Int] [2])
  print (run2 opsI 1 2)
  print (run2 opsN 1.5 2.5)
  print (run2 opsS "a" "b")
  print (run2 opsB (bsFromString "a") (bsFromString "b"))
  print (run2 opsBool True False)
  print (run2 mmI 1 2)
  print (run2 mmN 1.5 2.5)
  print (run2 mmS "a" "b")
  print (run2 mmB (bsFromString "a") (bsFromString "b"))
  putStrLn (deep 2 (7 :: Int))
  putStrLn (deepM 2 (7 :: Int))
  -- Reach every boxed-Integer runtime body (the Integer ops have no
  -- inline form, so each use below emits the named function).
  let gi = (1234567890123 :: Integer)
  print (gi + 2 * gi - abs gi + signum gi + negate gi)
  print (gi `div` 3, gi `mod` 3, gi `quot` 3, gi `rem` 3)
  print (divMod gi 3)
  print (quotRem gi 3)
  print (toInteger gi + toInteger (5 :: Int))
  print (gi == gi, gi < gi, gi > gi, gi <= gi, gi >= gi)
  print (max gi gi, min gi gi, compare gi gi)
"#;

/// One position's probe: `args` with a fresh bomb spliced at `bomb_at`.
fn call_args(args: &[&str], bomb_at: Option<usize>) -> String {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            if Some(i) == bomb_at { "__probe_bomb()".to_string() } else { (*a).to_string() }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The Lua block for one probe: existence, control call, one bombed call
/// per position with the recorded expectation.
fn probe_block(p: &Probe) -> String {
    let target = mllc::codegen::sanitize_name(p.name);
    let run = if p.run_action { " r = r()" } else { "" };
    let mut out = format!(
        "do\n    local target = {target}\n    if target == nil then\n        \
         __probe_fail(\"{name}: not reachable in the emitted runtime \
         (update the probe program)\")\n    else\n",
        name = p.name,
    );
    if !p.control_raises {
        out.push_str(&format!(
            "        do\n            local ok, err = pcall(function() local r = target({}){run} return r end)\n            \
             if not ok then __probe_fail(\"{}: control call raised: \" .. tostring(err)) end\n        end\n",
            call_args(p.args, None), p.name,
        ));
    }
    for (i, pos) in p.positions.iter().enumerate() {
        let call = format!(
            "local ok, err = pcall(function() local r = target({}){run} return r end)",
            call_args(p.args, Some(i)),
        );
        let check = match pos {
            Strict | ForcedOnRun => format!(
                "if ok then __probe_fail(\"{n}: position {i} is marked strict but the bomb did not raise\")\n            \
                 elseif not string.find(tostring(err), \"BOMB\", 1, true) then \
                 __probe_fail(\"{n}: position {i} raised something other than the bomb: \" .. tostring(err)) end",
                n = p.name,
            ),
            Lazy => format!(
                "if not ok then __probe_fail(\"{n}: position {i} is lazy but the bomb raised: \" .. tostring(err)) end",
                n = p.name,
            ),
        };
        out.push_str(&format!("        do\n            {call}\n            {check}\n        end\n"));
    }
    out.push_str("    end\nend\n");
    out
}

/// Coverage + behavior in one test: every mask entry has a probe of the
/// mask's exact shape, and every probe's recorded behavior holds against
/// the emitted runtime.
#[test]
fn strictness_masks_match_the_emitted_runtime() {
    // --- coverage: masks <-> probes, both directions ---
    let masks: std::collections::HashMap<&str, &[bool]> = mllc::demand::STRICT_BUILTINS
        .iter()
        .chain(mllc::demand::RUNTIME_PRELUDE_STRICTNESS)
        .map(|(n, m)| (*n, *m))
        .collect();
    let mut probes: Vec<Probe> = Vec::new();
    for p in PROBES {
        let mask = masks.get(p.name).unwrap_or_else(|| {
            panic!("probe '{}' has no mask entry in demand.rs — remove it", p.name)
        });
        assert_eq!(mask.len(), p.positions.len(),
            "'{}': mask arity {} != probe arity {}", p.name, mask.len(), p.positions.len());
        assert_eq!(p.args.len(), p.positions.len(),
            "'{}': sample count != arity", p.name);
        for (i, m) in mask.iter().enumerate() {
            let is_strict = p.positions[i] == Strict;
            assert_eq!(*m, is_strict,
                "'{}': mask position {} is {} but the probe records {}",
                p.name, i, m, if is_strict { "Strict" } else { "Lazy/ForcedOnRun" });
        }
        probes.push(Probe { ..*p });
    }
    let probed: std::collections::HashSet<&str> = PROBES.iter().map(|p| p.name).collect();
    for name in masks.keys() {
        assert!(probed.contains(name),
            "mask entry '{name}' has no behavioral probe — add one to PROBES");
    }
    for name in mllc::demand::PRIMITIVE_BINOP_METHODS {
        if PRIMITIVE_EXCLUSIONS.contains(name) {
            continue;
        }
        let (a, b) = primitive_samples(name);
        probes.push(Probe {
            name,
            args: Box::leak(Box::new([a, b])),
            positions: &[Strict, Strict],
            run_action: false,
            control_raises: false,
        });
    }

    // --- behavior: compile the probe program and bomb every position ---
    let lua_code = compile(PROBE_PROGRAM, Path::new("."), &[])
        .expect("the probe program must compile")
        .lua_code;
    let cut = lua_code.find("-- Entry point")
        .expect("entry point marker present");
    let mut probed_src = lua_code[..cut].to_string();
    probed_src.push_str(r#"
local __probe_failures = {}
local function __probe_fail(msg) __probe_failures[#__probe_failures + 1] = msg end
local function __probe_bomb() return __thunk(function() error("BOMB", 0) end) end
"#);
    for p in &probes {
        probed_src.push_str(&probe_block(p));
    }
    probed_src.push_str(
        "if #__probe_failures > 0 then \
         error(\"strictness contract violations:\\n\" .. table.concat(__probe_failures, \"\\n\"), 0) end\n",
    );
    let lua = mlua::Lua::new();
    lua.load(&probed_src).set_name("strictness_contract").exec()
        .expect("every strictness mask must match the emitted runtime body");
}
