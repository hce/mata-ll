//! Compile-and-run behavior tests: programs whose runtime output/behavior
//! is asserted in-process (laziness, shadowing, show rendering, layout, ...).

use super::*;

// Regression test: x <- return val must unwrap the thunk (was a known bug)
#[test]
fn bind_return_unwraps_value() {
    let source = r#"
main :: IO ()
main = do
    x <- return (10 :: Int)
    assert (x == 10) "bind return"
    putStrLn "ok"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("bind_return").exec()
        .expect("x <- return val should bind x to the value");
}

// Runtime error tests: these should compile but fail at runtime
#[test]
fn undefined_errors_when_forced() {
    // `x` carries a concrete type annotation. Without one, `print x` leaves the
    // element type of `x = undefined` unconstrained, which is a genuine ambiguous
    // type (GHC rejects `let x = undefined; print x` for the same reason); the
    // ambiguity check now flags it at compile time. The purpose of this test —
    // that a forced `undefined` raises `Prelude.undefined` at runtime — is
    // unchanged by pinning the type.
    let source = r#"
main :: IO ()
main = do
    let x = undefined :: Int
    print x
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("undefined should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    match lua.load(&lua_code).set_name("undefined_forced").exec() {
        Err(e) => {
            let msg = format!("{}", e);
            assert!(msg.contains("Prelude.undefined"),
                "Expected 'Prelude.undefined' error, got: {}", msg);
        }
        Ok(()) => panic!("Expected runtime error when forcing undefined"),
    }
}

// Examples that should compile successfully
#[test]
fn examples_compile() {
    let lib_path = Path::new("../lib");
    let examples_dir = Path::new("../experiments");

    // Examples expected to fail or skip
    let expected_fail: Vec<&str> = vec![
        "bench",              // show specialization gap on list display
        "aestest",            // 256-element S-box lists need large stack (runs via mll compiler)
        "bstest",             // needs large stack (runs via mll compiler)
        "salsa",              // large literal lists need large stack (runs via mll compiler)
        "Ed25519",            // large literal lists need large stack (runs via mll compiler)
        "ed25519test",        // depends on Ed25519 which needs large stack
        "metar",              // needs large stack (many nested parser combinators)
        "match",              // experimental scratch file
        "experiments",        // experimental scratch file
    ];

    let mut failures = Vec::new();
    for entry in std::fs::read_dir(examples_dir).expect("Cannot read experiments/") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "mll") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        if expected_fail.contains(&stem) {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));
        let source_dir = path.parent().unwrap_or(Path::new("."));
        match compile(&source, source_dir, &[lib_path]) {
            Ok(_) => {}
            Err(e) => failures.push(format!("{}: {}", stem, e)),
        }
    }
    if !failures.is_empty() {
        panic!("Examples failed to compile:\n{}", failures.join("\n"));
    }
}

// The curated showcases in examples/ must all compile. Some pull in the
// contrib library (atdg.mll uses Lz4/Hex), so the lib path carries both
// ../lib and ../contrib; the others ignore the extra path harmlessly.
#[test]
fn examples_curated_compile() {
    let lib = Path::new("../lib");
    let contrib = Path::new("../contrib");
    let examples_dir = Path::new("../examples");

    let mut failures = Vec::new();
    for entry in std::fs::read_dir(examples_dir).expect("Cannot read examples/") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "mll") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {}", path.display(), e));
        let source_dir = path.parent().unwrap_or(Path::new("."));
        if let Err(e) = compile(&source, source_dir, &[lib, contrib]) {
            failures.push(format!("{}: {}", stem, e));
        }
    }
    if !failures.is_empty() {
        panic!("Curated examples failed to compile:\n{}", failures.join("\n"));
    }
}

// Regression: a nullary constructor used as an argument of another pattern
// (e.g. `Box R n`, or the nested `T R (T R a x b) y c` in a red-black tree's
// balance) must parse. Previously the pattern-atom predicate omitted
// UpperIdent, so such arguments were rejected at parse time.
#[test]
fn nullary_constructor_as_pattern_argument() {
    let source = r#"
data Color = R | B
data Box = Box Color Int

unwrap :: Box -> Int
unwrap (Box R n) = n
unwrap (Box B n) = 0 - n

main :: IO ()
main = do
  print (unwrap (Box R 5))
  print (unwrap (Box B 5))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("nullary constructor as pattern arg should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("nullary_con_arg").exec()
        .expect("should run without error");
}

// Regression: a record field constructed from a non-cheap expression is stored
// as a thunk; projecting it in a strict (arithmetic) context must force it.
// Previously the accessor inlined to `v[idx]` without __force, so the thunk
// (a Lua table) reached arithmetic -> "arithmetic on a table value".
#[test]
fn record_field_projection_is_forced() {
    let source = r#"
data V = V { va :: Number, vb :: Number }

scaleV :: Number -> V -> V
scaleV s v = V (s * va v) (s * vb v)

dot :: V -> Number
dot v = va v * vb v

main :: IO ()
main = print (dot (scaleV 2.0 (V 5.0 7.0)))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("record_field_force").exec()
        .expect("projecting a thunk-valued field in arithmetic should force it");
}

// Layout: a function whose first argument is on the next line (no same-line
// argument) is consumed as an application. Previously the cross-line
// continuation required at least one same-line arg, so this failed inside
// parenthesized multi-line constructor application.
#[test]
fn first_argument_on_next_line() {
    let source = r#"
data T = L Int | N T T

deep :: T
deep = N (N (L 1)
            (L 2))
         (L 3)

size :: T -> Int
size (L _) = 1
size (N a b) = size a + size b

main :: IO ()
main = assert (size deep == 3) "function with first arg on next line"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("first arg on next line should parse")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("first_arg_next_line").exec()
        .expect("should run");
}

// Layout: a multi-line application-argument continuation indented past the
// enclosing block (but not necessarily past the function column) is now
// accepted, matching Haskell. Previously it required indentation past the
// function and was rejected as "Unexpected token at top level".
#[test]
fn shallow_multiline_continuation() {
    let source = r#"
import Data.List (foldl')

total :: Int
total = foldl' (\a b -> a + b) 0
  [1, 2, 3, 4, 5]

main :: IO ()
main = print total
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("shallow multi-line continuation should parse");
    let lua = mlua::Lua::new();
    lua.load(&lua_code.lua_code).set_name("shallow_cont").exec()
        .expect("should run");
}

// Regression: a self-referential lazy value bound in a `where` clause, an
// expression `let`, or a do-block `let` must close over itself. Two bugs
// combined here:
//   1. Codegen emitted `local x = __thunk(... x ...)`, but a Lua local is not
//      in scope within its own initializer, so the inner `x` resolved to a nil
//      global. The classic `fib = [1,1] ++ zipWith (+) fib (drop 1 fib)`
//      collapsed to `[1,1]`, so `fib !! 11` read as 1 instead of 144. Fixed by
//      forward-declaring the name (`local x`) before assigning it.
//   2. The typechecker treated `let`/do-`let` as sequential (let*), rejecting
//      self- and forward-references ("Unbound variable: fib"). Fixed by
//      inferring let groups as mutually recursive (pre-register fresh vars,
//      then generalize) — like `where`/top-level, but keeping let-polymorphism.
#[test]
fn recursive_lazy_value_in_where_let_and_do() {
    let source = r#"
fibTop :: [Int]
fibTop = [1, 1] ++ zipWith (+) fibTop (drop 1 fibTop)

nthWhere :: Int -> Int
nthWhere k = fib !! k
  where
    fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)

nthLet :: Int -> Int
nthLet k =
  let fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)
  in fib !! k

-- mutually recursive let bindings
isEven :: Int -> Bool
isEven n =
  let ev = \m -> if m == 0 then True else od (m - 1)
      od = \m -> if m == 0 then False else ev (m - 1)
  in ev n

-- let-polymorphism must survive the recursive-let change
polyPair :: (Int, Bool)
polyPair = let idf = \x -> x in (idf 5, idf True)

main :: IO ()
main = do
  let fibDo = [1, 1] ++ zipWith (+) fibDo (drop 1 fibDo)
  assert (fibTop !! 11 == 144) "top-level recursive list (12th fib)"
  assert (nthWhere 11 == 144) "where-bound recursive list (12th fib)"
  assert (nthWhere 12 == 233) "where-bound recursive list (13th fib)"
  assert (nthLet 11 == 144) "let-bound recursive list (12th fib)"
  assert (nthLet 12 == 233) "let-bound recursive list (13th fib)"
  assert (fibDo !! 11 == 144) "do-block let recursive list (12th fib)"
  assert (isEven 10) "mutually recursive let"
  assert (polyPair == (5, True)) "let-polymorphism preserved"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("recursive lazy where/let values should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("recursive_lazy_value").exec()
        .expect("recursive lazy where/let/do bindings should evaluate correctly");
}

// Regression: non-strict argument passing must keep lazy code productive.
// A user's prime sieve (a self-referential list filtered via a comprehension)
// diverged because several strict shortcuts leaked into lazy positions:
//   - a one-level function call passed as an argument (concatMap's recursion,
//     list comprehensions desugar to concatMap) was evaluated eagerly;
//   - `x : rest` force-evaluated a variable tail, collapsing the spine;
//   - lambda parameters were emitted bare and broke when a higher-order call
//     passed a thunk;
//   - a recursive call inside a guard was missed by the strictness analysis,
//     marking the parameter concrete while the call site thunked it.
// Each is exercised below. (Calls to inlinable helpers like makeAdder stay
// eager, so this must not regress arithmetic-heavy code.)
#[test]
fn lazy_arguments_and_infinite_lists() {
    let source = r#"
-- infinite list comprehension (desugars to concatMap) must stream
evens :: [Int]
evens = [x | x <- [1..], x `mod` 2 == 0]

-- a recursive call passed as a function argument must stay lazy
consit :: a -> [a] -> [a]
consit x rest = x : rest

countFrom :: Int -> [Int]
countFrom n = consit n (countFrom (n + 1))

-- foldr building a list: cons whose tail is a variable
copyList :: [Int] -> [Int]
copyList = foldr (\x acc -> x : acc) []

-- guard recursion with a thunked argument (the param is used strictly)
digitalRoot :: Int -> Int
digitalRoot n
  | n < 10    = n
  | otherwise = digitalRoot (digitSum n)
  where
    digitSum 0 = 0
    digitSum m = m `mod` 10 + digitSum (m `div` 10)

-- higher-order: a lambda param may arrive as a thunk and must be forced
makeAdder :: Int -> Int -> Int
makeAdder n = \x -> x + n

applyTwice :: (a -> a) -> a -> a
applyTwice f x = f (f x)

main :: IO ()
main = do
  assert (take 5 evens == [2, 4, 6, 8, 10]) "infinite list comprehension streams"
  assert (take 4 (countFrom 1) == [1, 2, 3, 4]) "recursive call as argument stays lazy"
  assert (copyList [1, 2, 3] == [1, 2, 3]) "foldr cons over a variable tail"
  assert (digitalRoot 493 == 7) "guard recursion with a thunked argument"
  assert (applyTwice (makeAdder 3) 0 == 6) "higher-order lambda param is forced"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("lazy-argument program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("lazy_arguments_and_infinite_lists").exec()
        .expect("lazy arguments and infinite lists should evaluate correctly");
}

// Prelude takeWhile / dropWhile, including the lazy case over an infinite list
// (takeWhile must stop without forcing the whole spine).
#[test]
fn prelude_take_while_drop_while() {
    let source = r#"
main :: IO ()
main = do
  assert (takeWhile (\x -> x < 4) [1, 2, 3, 4, 5] == [1, 2, 3]) "takeWhile finite"
  assert (takeWhile (\x -> x < 4) [1 ..] == [1, 2, 3]) "takeWhile infinite"
  assert (takeWhile (\x -> x < 10) [1, 2, 3] == [1, 2, 3]) "takeWhile exhausts"
  assert (takeWhile (\x -> x > 9) [1, 2, 3] == ([] :: [Int])) "takeWhile none"
  assert (dropWhile (\x -> x < 3) [1, 2, 3, 4, 5] == [3, 4, 5]) "dropWhile finite"
  assert (dropWhile (\x -> x > 9) [1, 2, 3] == [1, 2, 3]) "dropWhile none"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("takeWhile/dropWhile program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("prelude_take_while_drop_while").exec()
        .expect("takeWhile/dropWhile should evaluate correctly");
}

// Regression: a locally-bound name (function parameter, case-pattern var, or
// let-bound var) must shadow a same-named top-level/prelude function. The
// monomorphizer's specialization paths and the codegen Let/Case arms used to
// ignore locals, so e.g. `f elem = elem + 1` resolved `elem` to the prelude
// function instead of the parameter ("arithmetic on a function value").
#[test]
fn local_binding_shadows_prelude_function() {
    let source = r#"
-- parameter named like a prelude function (multi-clause, not inlined)
fParam :: Int -> Int
fParam 0 = 0
fParam elem = elem + 1

-- case-pattern variable named like a prelude function
fCase :: Maybe Int -> Int
fCase m = case m of
  Just reverse -> reverse + 1
  Nothing -> 0

-- let-bound variable named like a prelude function
fLet :: Int
fLet = let length = 41 in length + 1

main :: IO ()
main = do
  assert (fParam 10 == 11) "param shadows prelude fn"
  assert (fCase (Just 20) == 21) "case var shadows prelude fn"
  assert (fLet == 42) "let var shadows prelude fn"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("shadowing program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("local_binding_shadows_prelude_function").exec()
        .expect("local bindings should shadow prelude functions");
}

// The common Data.List helpers now live in the auto-imported Prelude, so they
// work with no `import Data.List`. (Data.List re-exports them, so explicit
// imports still work too — covered by lib_data_list and the examples.)
#[test]
fn prelude_list_helpers_without_import() {
    let source = r#"
main :: IO ()
main = do
  assert (null ([] :: [Int])) "null"
  assert (last [1, 2, 3] == 3) "last"
  assert (init [1, 2, 3] == [1, 2]) "init"
  assert (concat [[1, 2], [3]] == [1, 2, 3]) "concat"
  assert (replicate 3 7 == [7, 7, 7]) "replicate"
  assert (take 5 (iterate (\x -> x * 2) 1) == [1, 2, 4, 8, 16]) "iterate"
  assert (span (\x -> x < 3) [1, 2, 3, 4] == ([1, 2], [3, 4])) "span"
  assert (zip [1, 2, 3] [10, 20] == [(1, 10), (2, 20)]) "zip"
  assert (fst (unzip [(1, 10), (2, 20)]) == [1, 2]) "unzip fst"
  assert (and [True, True]) "and"
  assert (or [False, True]) "or"
  assert (any (\x -> x > 3) [1, 2, 4]) "any"
  assert (all (\x -> x > 0) [1, 2, 3]) "all"
  assert (sum [1, 2, 3, 4] == 10) "sum"
  assert (product [1, 2, 3, 4] == 24) "product"
  -- lazy over an infinite list (fst forces only the takeWhile half)
  assert (take 3 (fst (span (\x -> x < 100) [1 ..])) == [1, 2, 3]) "span lazy prefix"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("prelude list helpers should compile without import")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("prelude_list_helpers_without_import").exec()
        .expect("prelude list helpers should evaluate correctly");
}

// Regression: show must distinguish a tuple from a cons list by the cons
// metatable, not by shape. A 2-tuple whose second element is a list (e.g.
// `(1, [2, 3])`) was previously rendered as a cons cell, `[1, 2, 3]`.
#[test]
fn show_tuple_with_list_element() {
    let source = r#"
main :: IO ()
main = do
  assert (show (1, [2, 3]) == "(1,[2,3])") "tuple with list as second element"
  assert (show ([1, 2], [3, 4]) == "([1,2],[3,4])") "tuple of two lists"
  assert (show ([1, 2], 3) == "([1,2],3)") "tuple with list as first element"
  assert (show (1, 2) == "(1,2)") "plain tuple"
  -- An empty-list element must show as "[]", not the type-erased "Nothing"
  -- (the post-mono verifier flagged this latent tuple-show leak).
  assert (show ((1 :: Int), ([] :: [Int])) == "(1,[])") "tuple with empty list element"
  assert (show ((Just (1 :: Int)), (Nothing :: Maybe Int)) == "(Just 1,Nothing)") "tuple of Maybe elements"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("show_tuple_with_list_element").exec()
        .expect("show should distinguish tuples from lists");
}

// Regression: a `case` matching a nested pattern under a constructor whose
// payload is a thunk (built from a non-cheap expression) must force the field
// before destructuring it. Previously the inner pattern indexed into the raw
// thunk table, reading its internals (the `false` flag, a nil) as field values.
#[test]
fn case_nested_pattern_forces_thunked_field() {
    let source = r#"
data Pair = Pair (Int, Int)

slow :: Int -> Int
slow 0 = 0
slow n = slow (n - 1) + 1

mkPair :: Int -> Pair
mkPair x = Pair (slow x, slow x + 1)

main :: IO ()
main = case mkPair 3 of
         Pair (a, b) -> assert (a + b == 7) "nested pattern forces thunked field"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("nested_thunk_pat").exec()
        .expect("nested pattern under a thunked constructor payload should work");
}

// Regression: record field accessors are first-class. Previously they were
// only inlined at a direct `field r` application, so using one as a value
// (`map field xs`) or over-applying a function-typed field (`fnField r x`)
// referenced a non-existent global and failed.
#[test]
fn record_accessor_first_class() {
    let source = r#"
data R = R { rfn :: Int -> Int, rval :: Int }

applyAcc :: (R -> Int) -> R -> Int
applyAcc f r = f r

main :: IO ()
main = do
  let r = R (\y -> y + 1) 42
  -- accessor used as a higher-order value
  assert (applyAcc rval r == 42) "accessor passed as a value"
  -- accessor mapped over a list
  assert (sumList (map rval [R (\y -> y) 1, R (\y -> y) 2, R (\y -> y) 3]) == 6) "accessor mapped"
  -- over-applied function-typed field accessor: (rfn r) 10
  assert (rfn r 10 == 11) "over-applied function field accessor"

sumList :: [Int] -> Int
sumList [] = 0
sumList (x:xs) = x + sumList xs
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("first-class accessors should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("accessor_first_class").exec()
        .expect("first-class accessor uses should work");
}

// Compiler stress tests: larger, self-checking example programs that assert
// their own correctness at runtime (a failed roundtrip -> error -> test fail).
#[test]
fn example_huffman_roundtrip() {
    run_mll_file(Path::new("../experiments/huffman.mll"), &[Path::new("../lib")]);
}

#[test]
fn example_redblack_invariants() {
    run_mll_file(Path::new("../experiments/redblack.mll"), &[Path::new("../lib")]);
}

#[test]
fn example_scheme_eval() {
    run_mll_file(Path::new("../experiments/scheme.mll"), &[Path::new("../lib")]);
}

#[test]
fn example_raytracer_renders() {
    run_mll_file(Path::new("../experiments/raytracer.mll"), &[Path::new("../lib")]);
}

#[test]
fn example_typeinfer_checks() {
    run_mll_file(Path::new("../experiments/typeinfer.mll"), &[Path::new("../lib")]);
}

#[test]
fn example_listcomp() {
    run_mll_file(Path::new("../experiments/listcomp.mll"), &[Path::new("../lib")]);
}

#[test]
fn example_lambda_reduction() {
    run_mll_file(Path::new("../experiments/lambda.mll"), &[Path::new("../lib")]);
}

// Compile + run an `assert`-based program; a failed assert raises a Lua error,
// so exec() fails and the test fails (never passes vacuously).
fn assert_mll(stmts: &str) {
    let src = format!("main :: IO ()\nmain = do\n{stmts}\n");
    let lua = compile(&src, Path::new("."), &[])
        .unwrap_or_else(|e| panic!("compile failed:\n{e}"))
        .lua_code;
    let l = mlua::Lua::new();
    l.load(&lua).set_name("lambda_test").exec()
        .expect("program should run with all asserts holding");
}

// Regression battery for curried lambdas `\t -> \v -> …`. These compiled to
// nested 1-arg Lua functions but the call site applied every argument in one
// n-ary call, so surplus args were dropped and the inner function leaked out
// (`(\t -> \v -> t + v) 2 3` returned a function instead of 5). The fix flattens
// a lambda only in callee position, leaving argument-position lambdas curried.

#[test]
fn curried_lambda_full_application() {
    assert_mll("    assert ((\\t -> \\v -> t + v) 2 3 == 5) \"full app\"");
}

#[test]
fn curried_lambda_partial_then_apply() {
    assert_mll("    let g = (\\a -> \\b -> a + b) 10\n    assert (g 5 == 15) \"partial\"");
}

#[test]
fn curried_lambda_triple_full() {
    assert_mll("    assert ((\\a -> \\b -> \\c -> a + b + c) 1 2 3 == 6) \"triple full\"");
}

#[test]
fn curried_lambda_triple_partial() {
    assert_mll("    let g = (\\a -> \\b -> \\c -> a + b + c) 1\n    assert (g 2 3 == 6) \"triple partial\"");
}

#[test]
fn curried_lambda_parenthesized_inner() {
    assert_mll("    assert ((\\t -> (\\v -> t - v)) 10 3 == 7) \"paren inner\"");
}

#[test]
fn curried_lambda_four_levels() {
    assert_mll("    assert ((\\a -> \\b -> \\c -> \\d -> a + b + c + d) 1 2 3 4 == 10) \"four levels\"");
}

#[test]
fn curried_lambda_captures_outer_binding() {
    assert_mll("    let k = 100\n    assert ((\\a -> \\b -> a + b + k) 1 2 == 103) \"capture\"");
}

#[test]
fn curried_lambda_non_integer_result() {
    // const-like: returns the first argument, ignores the second
    assert_mll("    assert ((\\s -> \\n -> s) \"hi\" (5 :: Int) == \"hi\") \"const\"");
}

#[test]
fn curried_lambda_returns_list() {
    assert_mll("    assert ((\\x -> \\y -> [x, y]) (1 :: Int) 2 == [1, 2]) \"list result\"");
}

#[test]
fn curried_lambda_embedded_in_expression() {
    assert_mll("    assert (((\\a -> \\b -> a * b) 6 7) + 1 == 43) \"embedded\"");
}

#[test]
fn curried_lambda_takes_function_argument() {
    // Higher-order *and* curried: first parameter is itself a function.
    assert_mll("    assert ((\\f -> \\x -> f x + 1) (\\y -> y * 2) 10 == 21) \"fn arg\"");
}

#[test]
fn curried_lambda_in_higher_order_stays_curried() {
    // The complementary case a naive flatten would break: the erased runtime
    // `map` applies its function argument to ONE argument and expects a
    // function back. Lambdas are flattened to their full type arity, so the
    // compiler must wrap arguments to map/zipWith in a currying adapter
    // (__mll_curry1/2) whenever the result type variable is instantiated to a
    // function type.
    let src = r#"
applyAll :: [a -> b] -> a -> [b]
applyAll []     _ = []
applyAll (f:fs) x = f x : applyAll fs x

main :: IO ()
main = do
    let fns = map (\n -> \x -> x + n) [1, 5, 10]
    assert (applyAll fns 42 == [43, 47, 52]) "higher-order curried"
"#;
    let lua = compile(src, Path::new("."), &[]).expect("compile").lua_code;
    let l = mlua::Lua::new();
    l.load(&lua).set_name("ho_curried").exec().expect("higher-order curried lambda should work");
}

#[test]
fn str_to_ints_unpacks_char_codes() {
    // strToInts bridges mata-ll's opaque String to a list of character codes,
    // in order. A wrong result aborts the program via `error`, failing exec().
    let source = r#"
import LString (strToInts)

main :: IO ()
main = do
    if strToInts "AZ" == [65, 90]
        then pure () else error "AZ codes wrong"
    if strToInts "hello" == [104, 101, 108, 108, 111]
        then pure () else error "hello codes wrong"
"#;
    let lua_code = compile(source, Path::new("."), &[Path::new("../lib")])
        .expect("strToInts program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code).set_name("str_to_ints").exec()
        .expect("strToInts should produce the expected character codes");
}

#[test]
fn print_of_empty_list_shows_brackets_not_nothing() {
    // Regression: [] and Nothing share a runtime rep (Lua nil). `print` used the
    // type-erased generic show, which guessed "Nothing" for nil — so an empty
    // [Int] (even nested) printed as "Nothing". `print` must use the typed
    // list show (which knows nil means []), while real Nothing still shows.
    let source = r#"
main :: IO ()
main = do
    print ([] :: [Int])
    print ([[1, 2], []] :: [[Int]])
    print (Nothing :: Maybe Int)
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    // Capture `print` output instead of letting it hit stdout.
    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name("print_empty").exec()
        .expect("should run");

    let lines: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(lines, vec!["[]", "[[1,2],[]]", "Nothing"]);
}

#[test]
fn derived_show_uses_constructor_names_and_parens() {
    // Regression: derived Show must render constructor names (not numeric tags
    // or tuples), recurse through polymorphic types (Tree a b / Box a), and
    // parenthesize constructor-application fields like GHC (showsPrec 11):
    // nullary/atomic fields stay bare, negatives get parens.
    let source = r#"
data Tree a b = Leaf a b | Branch (Tree a b) (Tree a b) deriving (Show)
data Box a = MkBox a deriving (Show)
data C = Red | Green deriving (Show)
data P a = P a a deriving (Show)
data B = MkB Int deriving (Show)

main :: IO ()
main = do
    print (Branch (Leaf (1 :: Int) (2 :: Int)) (Leaf 3 4))
    print (MkBox (MkBox (5 :: Int)))
    print (P Red Green)
    print (MkB (0 - 5))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name("derived_show").exec()
        .expect("should run");

    let lines: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(
        lines,
        vec![
            "Branch (Leaf 1 2) (Leaf 3 4)", // polymorphic recursion + parens
            "MkBox (MkBox 5)",              // nested poly constructor
            "P Red Green",                  // nullary fields: no parens
            "MkB (-5)",                     // negative: parens
        ]
    );
}

#[test]
fn show_maybe_renders_just() {
    // Regression: `show` renders the Maybe structure. `Just` is now an injective
    // tagged wrapper, so `Just Nothing` is distinct from `Nothing` at every
    // nesting level and renders "Just Nothing" (it no longer collapses to nil).
    let source = r#"
main :: IO ()
main = do
    print (Just (5 :: Int))
    print (Nothing :: Maybe Int)
    print (Just (Just (5 :: Int)))
    print (Just (0 - 5 :: Int))
    print [Just (1 :: Int), Nothing, Just 3]
    print (Just (Nothing :: Maybe Int))
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name("show_maybe").exec()
        .expect("should run");

    let lines: Vec<String> = captured
        .sequence_values::<String>()
        .collect::<mlua::Result<_>>()
        .unwrap();
    assert_eq!(
        lines,
        vec![
            "Just 5",
            "Nothing",
            "Just (Just 5)",
            "Just (-5)",
            "[Just 1,Nothing,Just 3]",
            "Just Nothing", // injective Just: distinct from Nothing
        ]
    );
}

// Helper: compile + run, capturing `print`/`putStrLn` output lines.
fn run_capturing_lines(source: &str, name: &str) -> Vec<String> {
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let captured = lua.create_table().unwrap();
    lua.globals().set("__captured", captured.clone()).unwrap();
    let print_fn = lua
        .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
            let line = s.to_str()?.to_string();
            let t: mlua::Table = lua.globals().get("__captured")?;
            let n = t.raw_len();
            t.raw_set(n + 1, line)?;
            Ok(())
        })
        .unwrap();
    lua.globals().set("print", print_fn).unwrap();
    lua.load(&lua_code).set_name(name).exec().expect("should run");
    captured.sequence_values::<String>().collect::<mlua::Result<_>>().unwrap()
}

#[test]
fn nested_maybe_is_injective() {
    // `Just` is an injective tagged wrapper: `Just Nothing` is distinct from
    // `Nothing`, `Just (Just x)` from `Just x`, at every nesting level — with
    // correct show, ==, and pattern-matching (via the Data.Maybe functions).
    let source = r#"
import qualified Data.Maybe as M

main :: IO ()
main = do
    putStrLn (show (Nothing :: Maybe (Maybe Int)))
    putStrLn (show (Just Nothing :: Maybe (Maybe Int)))
    putStrLn (show (Just (Just 5) :: Maybe (Maybe Int)))
    putStrLn (show ((Just Nothing :: Maybe (Maybe Int)) == Nothing))
    putStrLn (show ((Just Nothing :: Maybe (Maybe Int)) == Just Nothing))
    putStrLn (show ((Just (Just 5) :: Maybe (Maybe Int)) == Just (Just 5)))
    putStrLn (show (M.isJust (Just Nothing :: Maybe (Maybe Int))))
    putStrLn (show (M.isNothing (Just Nothing :: Maybe (Maybe Int))))
    putStrLn (show (M.fromJust (Just (Just 7)) :: Maybe Int))
    putStrLn (show (M.fromMaybe (Just 9) (Just Nothing :: Maybe (Maybe Int))))
    putStrLn (show (M.maybe 0 (M.fromMaybe 1) (Just (Just 8) :: Maybe (Maybe Int))))
"#;
    let lines = run_capturing_lines(source, "nested_maybe");
    assert_eq!(
        lines,
        vec![
            "Nothing",         // Nothing :: Maybe (Maybe Int)
            "Just Nothing",    // distinct from Nothing
            "Just (Just 5)",
            "False",           // Just Nothing /= Nothing
            "True",            // Just Nothing == Just Nothing
            "True",            // Just (Just 5) == Just (Just 5)
            "True",            // isJust (Just Nothing)
            "False",           // isNothing (Just Nothing)
            "Just 7",          // fromJust (Just (Just 7))
            "Nothing",         // fromMaybe (Just 9) (Just Nothing) = the inner Nothing
            "8",               // maybe 0 (fromMaybe 1) (Just (Just 8))
        ]
    );
}

#[test]
fn just_of_empty_list_distinct_from_nothing() {
    // `[]` is also nil at runtime, so `Just []` used to collapse to Nothing too.
    // The wrapper keeps them distinct.
    let source = r#"
main :: IO ()
main = do
    putStrLn (show (Just [] :: Maybe [Int]))
    putStrLn (show (Nothing :: Maybe [Int]))
    putStrLn (show ((Just [] :: Maybe [Int]) == Nothing))
    putStrLn (show (Just [1, 2] :: Maybe [Int]))
"#;
    let lines = run_capturing_lines(source, "just_empty_list");
    assert_eq!(lines, vec!["Just []", "Nothing", "False", "Just [1,2]"]);
}

#[test]
fn lazy_index_elements_print_as_values() {
    // Finding 1, exact repro on the PRINT path: an element pulled from a
    // lazily-generated list via head/tail/(!!) must print as its value.
    // Before the fix, a raw thunk escaped and `print` rendered its Lua
    // representation ("(function: 0x.., False)" / garbage), and the
    // let-bound form crashed with "attempt to perform arithmetic on a table
    // value". Asserting on the captured output catches the leak even when
    // it does NOT crash.
    let source = r#"
inc :: Int -> Int
inc x = x + 1

main :: IO ()
main = do
    print (head (tail (iterate inc 0)))
    print ([1..] !! 5)
    let v = iterate inc 0 !! 2
    print (v * 10)
    print (take 3 (iterate inc 0))
"#;
    let lines = run_capturing_lines(source, "lazy_index_print");
    assert_eq!(
        lines,
        vec![
            "1",         // head (tail (iterate inc 0)) — leaked "(function: 0x.., False)"
            "6",         // [1..] !! 5 — index 5 of [1,2,3,...] is 6; printed garbage pre-fix
            "20",        // (iterate inc 0 !! 2) * 10 — crashed on arithmetic
            "[0,1,2]", // take must materialize values, not thunks
        ]
    );
}

// Regression: the entry-point trailer used to run main() only when the chunk's
// first vararg was nil. A standalone interpreter (`lua prog.lua x`) passes CLI
// args as varargs, so ANY argument made the program look like it had been
// `require`d and main was silently skipped. main must run whenever the file is
// executed as a program (first vararg matches arg[1], including the no-arg case
// where both are absent) and stay dormant only when a host require()s it (first
// vararg is the module name, which won't match arg[1]). On ambiguity we err
// toward running main: a genuine library module carries no main to begin with.
#[test]
fn main_runs_standalone_with_cli_args_not_when_required() {
    let source = r#"
import LIO (putStrLn)

main :: IO ()
main = do
    args <- getArgs
    putStrLn "MAIN"
    putStrLn (show args)
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    // Exec the chunk the way a standalone interpreter would: `arg` set as a
    // global (arg[0]=script, arg[1..]=CLI args) and the same args handed to the
    // chunk as varargs. `arg1` is arg[1]; `first_vararg` is the chunk's `...`.
    let run = |arg1: Option<&str>, first_vararg: &str| -> Vec<String> {
        let lua = mlua::Lua::new();
        let captured = lua.create_table().unwrap();
        lua.globals().set("__captured", captured.clone()).unwrap();
        let print_fn = lua
            .create_function(|lua, s: mlua::LuaString| -> mlua::Result<()> {
                let t: mlua::Table = lua.globals().get("__captured")?;
                let n = t.raw_len();
                t.raw_set(n + 1, s.to_str()?.to_string())?;
                Ok(())
            })
            .unwrap();
        lua.globals().set("print", print_fn).unwrap();
        let arg_tbl = lua.create_table().unwrap();
        arg_tbl.raw_set(0, "prog.lua").unwrap();
        if let Some(a) = arg1 {
            arg_tbl.raw_set(1, a).unwrap();
        }
        lua.globals().set("arg", arg_tbl).unwrap();
        lua.load(&lua_code)
            .set_name("entrypoint")
            .call::<()>(first_vararg.to_string())
            .expect("chunk runs");
        captured
            .sequence_values::<String>()
            .collect::<mlua::Result<_>>()
            .unwrap()
    };

    // Standalone with a CLI argument: first vararg == arg[1] == "alpha" → run.
    assert_eq!(run(Some("alpha"), "alpha"), vec!["MAIN", "[\"alpha\"]"]);

    // Required for its exports: first vararg is the module name "prog" while the
    // host passed no args (arg[1] unset), so they differ → main stays dormant.
    assert!(
        run(None, "prog").is_empty(),
        "main must not run when the module is require()d"
    );
}
