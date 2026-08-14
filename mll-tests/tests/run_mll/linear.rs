//! Linear types (`%1` / `%m`): rejection tests for the exactly-once
//! discipline, and the erasure guarantees.

use super::*;

// ---------------------------------------------------------------------------
// Linear types: `a %1 -> b` — a `%1` value must be consumed EXACTLY once
// (GHC LinearTypes semantics: more than one use is a double-free, zero uses
// is a leak). The positive side (programs that use `%1` correctly compile
// and run, and the annotation erases) lives in linear_affine_basic.mll;
// the tests here assert REJECTION — a program that can use a `%1`-bound
// value more than once, or drop it, must fail to compile with a diagnostic
// that names the variable and explains the violation in plain language.
// See mllc/src/typechecker/usage.rs for the enforced fragment.
// ---------------------------------------------------------------------------

/// Compile expecting a linearity rejection; return the rendered error.
fn expect_linear_reject(src: &str) -> String {
    match compile(src, Path::new("tests/cases"), &[]) {
        Ok(_) => panic!(
            "this program violates the %1 (exactly-once) discipline and \
             must NOT compile:\n{}",
            src
        ),
        Err(e) => format!("{}", e),
    }
}

/// The simplest violation: a `%1` argument mentioned twice.
#[test]
fn linear_rejects_plain_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         dup :: Token %1 -> (Token, Token)\n\
         dup t = (t, t)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("declares this argument '%1'"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// Passing a `%1` value to an unrestricted function is an over-use even when
/// it occurs only once: the callee's plain arrow makes no single-use promise.
#[test]
fn linear_rejects_flow_into_unrestricted_function() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         count :: Token -> Int\n\
         count (Token n) = n\n\
         g :: Token %1 -> Int\n\
         g t = count t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("passed to 'count'"), "{}", msg);
    assert!(msg.contains("'->', not '%1 ->'"), "{}", msg);
}

/// Aliasing through a pattern match: the binder inherits the restriction.
#[test]
fn linear_rejects_case_alias_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         data Box = Box Token\n\
         f :: Box %1 -> (Token, Token)\n\
         f b = case b of\n\
         \x20 Box t -> (t, t)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("pattern-bound from 'b'"), "{}", msg);
}

/// Aliasing through `let`: using the alias twice consumes the original twice
/// (the laziness rule — the thunk memoizes the FORCE, not the consumption).
#[test]
fn linear_rejects_let_alias_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> (Token, Token)\n\
         f t = let u = t in (u, u)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("local binding 'u'"), "{}", msg);
}

/// Capture by a returned closure: the closure may be called any number of
/// times, each call handing out the same `%1` value again.
#[test]
fn linear_rejects_closure_capture() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> (Int -> Token)\n\
         f t = \\x -> t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("captured by a function value"), "{}", msg);
}

/// The propagation soundness case: a lambda checked against a `%1`
/// parameter learns the restriction through unification and its binder is
/// enforced — an ω-style lambda cannot sneak in through a %1 HOF.
#[test]
fn linear_rejects_duplicating_lambda_at_linear_hof() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         withToken :: (Token %1 -> (Token, Token)) -> (Token, Token)\n\
         withToken f = f (Token 1)\n\
         main :: IO ()\n\
         main = case withToken (\\t -> (t, t)) of\n\
         \x20 (Token a, Token b) -> print (a + b)\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("'%1' arrow at this parameter"), "{}", msg);
}

/// A named unrestricted function cannot flow into a `%1` position at all —
/// the arrows are different types (invariant multiplicities, as in GHC).
#[test]
fn linear_rejects_unrestricted_function_at_linear_type() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         applyMany :: (Token -> Int) -> Int\n\
         applyMany f = f (Token 1) + f (Token 2)\n\
         main :: IO ()\n\
         main = print (applyMany useOnce)\n",
    );
    assert!(msg.contains("arrows disagree"), "{}", msg);
    assert!(msg.contains("exactly once"), "{}", msg);
}

/// Sequential double use across a do-block: `>>`-chained statements add up.
#[test]
fn linear_rejects_double_use_across_do_block() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         shred :: Token %1 -> IO ()\n\
         shred (Token n) = print n\n\
         f :: Token %1 -> IO ()\n\
         f t = do\n\
         \x20 shred t\n\
         \x20 shred t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
}

/// A `<-` binder aliasing an affine value inherits the restriction.
#[test]
fn linear_rejects_bind_alias_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         data Box = Box Token\n\
         unbox :: Box %1 -> Token\n\
         unbox (Box t) = t\n\
         shred :: Token %1 -> IO ()\n\
         shred (Token n) = print n\n\
         f :: Box %1 -> IO ()\n\
         f b = do\n\
         \x20 t <- pure (unbox b)\n\
         \x20 shred t\n\
         \x20 shred t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("bound (with '<-')"), "{}", msg);
}

/// A locally shadowed Prelude name must not inherit the Prelude's
/// consume-once whitelisting (`pure`, `id`, `fst`, …).
#[test]
fn linear_rejects_shadowed_prelude_whitelist_name() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> (Token, Token)\n\
         f t = let pure = \\x -> (x, x) in pure t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
}

/// A `%1` class-method signature is enforced on instance methods too.
#[test]
fn linear_rejects_instance_method_double_use() {
    let msg = expect_linear_reject(
        "data Pair = Pair Int Int\n\
         data Token = Token Pair\n\
         class Consume a where\n\
         \x20 consume :: a %1 -> (a, a)\n\
         instance Consume Token where\n\
         \x20 consume t = (t, t)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("must be consumed exactly once"), "{}", msg);
}

/// Erasure: multiplicities are a type-checking discipline only. The same
/// program with `%1` arrows and with plain arrows must emit byte-identical
/// Lua.
#[test]
fn linear_annotations_erase_to_identical_lua() {
    let with_mult = "data Token = Token Int\n\
         shred :: Token %1 -> IO ()\n\
         shred (Token n) = if n == 42 then putStrLn \"ok\" else putStrLn \"bad\"\n\
         step :: Token %1 -> (Token, Int)\n\
         step t = (t, 5)\n\
         main :: IO ()\n\
         main = do\n\
         \x20 let t = Token 42\n\
         \x20 case step t of\n\
         \x20\x20 (t2, n) -> do\n\
         \x20\x20\x20 print n\n\
         \x20\x20\x20 shred t2\n";
    let without_mult = with_mult.replace("%1 ->", "->");
    let a = compile(with_mult, Path::new("tests/cases"), &[])
        .expect("the %1 program must compile")
        .lua_code;
    let b = compile(&without_mult, Path::new("tests/cases"), &[])
        .expect("the plain-arrow program must compile")
        .lua_code;
    assert!(a == b, "%1 must erase: emitted Lua differs");
}

// ---------------------------------------------------------------------------
// Multiplicity polymorphism (`a %m -> b`) and the composability relaxations
// (local-function forwarding, non-IO binds). The positive side lives in
// linear_mult_poly.mll; these assert that a double use which is only
// reachable THROUGH a polymorphic helper, a local function, or a non-IO
// bind is still rejected, and that a polymorphic definition is held to the
// `m = 1` instantiation.
// ---------------------------------------------------------------------------

/// A definition polymorphic in `m` may not duplicate its `%m` argument:
/// a caller can instantiate m to 1.
#[test]
fn linear_rejects_double_use_in_mult_poly_definition() {
    let msg = expect_linear_reject(
        "dupPoly :: a %m -> (a, a)\n\
         dupPoly x = (x, x)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'x' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("multiplicity variable '%m'"), "{}", msg);
    assert!(msg.contains("instantiate to '%1'"), "{}", msg);
}

/// A `%1` binder passed through the polymorphic helper twice is still two
/// uses — polymorphism must not launder the count.
#[test]
fn linear_rejects_double_use_through_mult_poly_helper() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         apply :: (a %m -> b) -> a %m -> b\n\
         apply f x = f x\n\
         bad :: Token %1 -> Int\n\
         bad t = apply useOnce t + apply useOnce t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// A duplicating lambda slipped through the polymorphic helper leaves `m`
/// unresolved, so the argument is charged unrestrictedly — reject.
#[test]
fn linear_rejects_duplicating_lambda_through_mult_poly_helper() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         apply :: (a %m -> b) -> a %m -> b\n\
         apply f x = f x\n\
         bad :: Token %1 -> (Token, Token)\n\
         bad t = apply (\\u -> (u, u)) t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
}

/// A `%1` value may not flow into a `%m` arrow position: the variable may
/// be instantiated to Many by the caller.
#[test]
fn linear_rejects_linear_arg_at_mult_var_arrow() {
    let msg = expect_linear_reject(
        "cross :: (a %m -> b) -> a %1 -> b\n\
         cross f x = f x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'x' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("signature variable ('%m')"), "{}", msg);
}

/// A rigid `%m` cannot be pinned to `Many` by the body (here by passing the
/// `%m` function to an unrestricted higher-order function) — the signature's
/// polymorphism claim would be silently broken for `m = 1` callers.
#[test]
fn linear_rejects_rigid_mult_weakened_to_many() {
    let msg = expect_linear_reject(
        "twice :: (c -> d) -> c -> d\n\
         twice g y = g y\n\
         force :: (a %m -> b) -> a %m -> b\n\
         force f x = twice f x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("arrows disagree"), "{}", msg);
    assert!(msg.contains("multiplicity VARIABLE"), "{}", msg);
}

/// Laundering a `%m` function through a local alias into a `%1` context
/// must not work either: the alias keeps the SAME rigid m.
#[test]
fn linear_rejects_mult_var_alias_laundering() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         dup2 :: Token -> (Token, Token)\n\
         dup2 t = (t, t)\n\
         h :: (c %1 -> d) -> c %1 -> d\n\
         h k y = k y\n\
         bad :: (a %m -> b) -> a %1 -> b\n\
         bad f x = let g = f in h g x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("arrows disagree"), "{}", msg);
}

/// A local function that uses its parameter twice makes the call a double
/// use of the affine argument.
#[test]
fn linear_rejects_local_function_param_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Int\n\
         bad t = g t\n\
         \x20 where g x = useOnce x + useOnce x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local function 'g'"), "{}", msg);
}

/// Calling a (correctly forwarding) local function twice is two uses.
#[test]
fn linear_rejects_double_call_of_forwarding_local_function() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Int\n\
         bad t = let g x = useOnce x in g t + g t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// A recursive local function that consumes the forwarded value on the way
/// down AND at the end: caught by the group fixpoint.
#[test]
fn linear_rejects_recursive_local_function_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Int -> Int\n\
         bad t n = go t n\n\
         \x20 where go x k = if k > 0 then go x (k - 1) + useOnce x else useOnce x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local function 'go'"), "{}", msg);
}

/// CAPTURING an affine value in a local function still charges ω — only the
/// function's parameters get the refined accounting, a returned closure may
/// be called any number of times.
#[test]
fn linear_rejects_local_function_capture() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         bad :: Token %1 -> (Int -> Token)\n\
         bad t = g\n\
         \x20 where g x = t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("called any number of times"), "{}", msg);
}

/// A double use inside a Maybe do-block is still two uses — the bind
/// relaxation only stops the blanket ω-charge, not the counting.
#[test]
fn linear_rejects_double_use_in_maybe_do_block() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Maybe Int\n\
         bad t = do\n\
         \x20 a <- Just (useOnce t)\n\
         \x20 b <- Just (useOnce t)\n\
         \x20 pure (a + b)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// The LIST bind runs its continuation once per element: an affine value
/// consumed in it stays rejected.
#[test]
fn linear_rejects_affine_in_list_monad_bind() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> [Int]\n\
         bad t = do\n\
         \x20 n <- [1, 2, 3]\n\
         \x20 pure (useOnce t + n)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("any number of times"), "{}", msg);
}

/// A USER-DEFINED monad's bind is arbitrary code (this one really does run
/// the continuation twice): its continuations stay ω-charged.
#[test]
fn linear_rejects_affine_in_user_monad_bind() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         data Twice a = Twice a a\n\
         instance Functor Twice where\n\
         \x20 fmap f (Twice a b) = Twice (f a) (f b)\n\
         instance Applicative Twice where\n\
         \x20 pure x = Twice x x\n\
         \x20 (<*>) (Twice f g) (Twice a b) = Twice (f a) (g b)\n\
         instance Monad Twice where\n\
         \x20 (>>=) (Twice a b) k = case k a of\n\
         \x20\x20 Twice x _ -> case k b of\n\
         \x20\x20\x20 Twice _ y -> Twice x y\n\
         bad :: Token %1 -> Twice Int\n\
         bad t = do\n\
         \x20 n <- Twice 1 2\n\
         \x20 pure (useOnce t + n)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("any number of times"), "{}", msg);
}

/// Erasure for the new pieces: `%m` annotations (and the `%1`s they compose
/// with) must emit byte-identical Lua to the plain-arrow program.
#[test]
fn linear_mult_poly_erases_to_identical_lua() {
    let with_mult = "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         apply :: (a %m -> b) -> a %m -> b\n\
         apply f x = f x\n\
         go :: Token %1 -> Int\n\
         go t = apply useOnce t\n\
         main :: IO ()\n\
         main = print (go (Token 3))\n";
    let without_mult = with_mult.replace("%1 ->", "->").replace("%m ->", "->");
    let a = compile(with_mult, Path::new("tests/cases"), &[])
        .expect("the %m program must compile")
        .lua_code;
    let b = compile(&without_mult, Path::new("tests/cases"), &[])
        .expect("the plain-arrow program must compile")
        .lua_code;
    assert!(a == b, "%m must erase: emitted Lua differs");
}

// ---------------------------------------------------------------------------
// The exactly-once LOWER bound: a `%1` value consumed zero times — dropped
// outright, dropped on one evaluation path, or parked in something that is
// never forced — is a leak and must be rejected. (The affine upper bound
// alone accepted all of these.)
// ---------------------------------------------------------------------------

/// The simplest leak: a `%1` argument never used at all.
#[test]
fn linear_rejects_zero_uses() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> Int\n\
         f t = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// A wildcard argument pattern discards the `%1` value without consuming it.
#[test]
fn linear_rejects_wildcard_argument_pattern() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         f :: Token %1 -> Int\n\
         f _ = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("wildcard"), "{}", msg);
}

/// Consumed in one case alternative but not its sibling: the sibling path
/// drops it. This is the lower-bound side of the branch join — the
/// per-variable maximum alone would still read "one use".
#[test]
fn linear_rejects_use_in_one_branch_only() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Int -> Int\n\
         f t n = case n > 0 of\n\
         \x20 True -> useOnce t\n\
         \x20 False -> 1\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("only 1 of the 2 alternatives"), "{}", msg);
}

/// The `if` form of the same lower bound.
#[test]
fn linear_rejects_use_in_one_if_arm_only() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Int -> Int\n\
         f t n = if n > 0 then useOnce t else 1\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("branches of this 'if'"), "{}", msg);
}

/// The laziness case: a `%1` value consumed only inside a `let` binding
/// that is never forced. The binding's right-hand side is scaled by its
/// use count (zero), so at clause end the value was never consumed.
#[test]
fn linear_rejects_never_forced_let_binding() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Int\n\
         f t = let u = useOnce t in 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// The same through a `where` binding.
#[test]
fn linear_rejects_never_forced_where_binding() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Int\n\
         f t = 5\n\
         \x20 where u = useOnce t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// Forwarded through the multiplicity-polymorphic helper and THEN dropped:
/// exactly-once must hold end to end, not just at the forwarding step.
#[test]
fn linear_rejects_drop_after_mult_poly_forward() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         apply :: (a %m -> b) -> a %m -> b\n\
         apply f x = f x\n\
         bad :: Token %1 -> Int\n\
         bad t = let u = apply useOnce t in 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// A `%m` binder must be consumed too: a caller may instantiate m to 1,
/// and multiplicity 1 demands consumption.
#[test]
fn linear_rejects_unused_mult_var_argument() {
    let msg = expect_linear_reject(
        "dropPoly :: a %m -> Int\n\
         dropPoly x = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'x' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("multiplicity variable '%m'"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// Maybe's bind skips its continuation on Nothing, so a `%1` value consumed
/// inside the continuation leaks on that path (GHC agrees: Maybe's bind
/// cannot promise to run a linear continuation). Consume it in the bind's
/// ACTION instead — see viaMaybe in linear_mult_poly.mll.
#[test]
fn linear_rejects_consumption_in_maybe_continuation() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Maybe Int\n\
         bad t = do\n\
         \x20 n <- Just 1\n\
         \x20 pure (useOnce t + n)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("'Nothing' path skips"), "{}", msg);
}

/// Discarding a non-`()` result whose thunk may hold the pending
/// consumption (`pure (useOnce t)` never forces the payload; running the
/// action does not consume t — only forcing the result would).
#[test]
fn linear_rejects_discarded_tainted_bind_result() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> IO ()\n\
         f t = do\n\
         \x20 _ <- pure (useOnce t)\n\
         \x20 putStrLn \"x\"\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("discarded"), "{}", msg);
}

/// The bare-statement (`>>`) form of the same discard.
#[test]
fn linear_rejects_discarded_tainted_statement_result() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> IO ()\n\
         f t = do\n\
         \x20 pure (useOnce t)\n\
         \x20 putStrLn \"x\"\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("discarded"), "{}", msg);
}

/// A wildcard inside a pattern over a `%1` value discards the matched part.
#[test]
fn linear_rejects_wildcard_in_tainted_case() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: (Token, Token) %1 -> Int\n\
         f p = case p of\n\
         \x20 (a, _) -> useOnce a\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'p' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("wildcard"), "{}", msg);
}

/// A local function that never uses its parameter drops the forwarded
/// value: the inferred per-parameter factors carry a may-drop flag.
#[test]
fn linear_rejects_dropping_local_function() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         bad :: Token %1 -> Int\n\
         bad t = g t\n\
         \x20 where g x = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local function 'g'"), "{}", msg);
    assert!(msg.contains("never uses this parameter"), "{}", msg);
}

/// `&&`/`||` short-circuit: the right operand may never run.
#[test]
fn linear_rejects_short_circuit_right_operand() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         f :: Token %1 -> Bool -> Bool\n\
         f t b = b && (useOnce t > 0)\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("short-circuits"), "{}", msg);
}

/// `fst` drops the second component: under exactly-once it is no longer a
/// consume-once function (its arrow is unrestricted, as in GHC).
#[test]
fn linear_rejects_fst_on_linear_pair() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         g :: (Token, Token) %1 -> Token\n\
         g p = fst p\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'p' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("passed to 'fst'"), "{}", msg);
}

/// A scalar destructured from a `%1` match is tracked exactly-once like
/// any other alias (GHC parity — no scalar exemption): the callee may have
/// parked the consumption in that component's thunk
/// (`step t = (Token 0, useOnce t)` — dropping n means t is never used).
#[test]
fn linear_rejects_unused_scalar_alias() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         step :: Token %1 -> (Token, Int)\n\
         step t = (Token 0, useOnce t)\n\
         f :: Token %1 -> Int\n\
         f t = case step t of\n\
         \x20 (t2, n) -> useOnce t2\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'n' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// The clause-pattern form: an unused scalar field of a `%1` argument.
#[test]
fn linear_rejects_unused_scalar_field() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         sig :: Token %1 -> Int\n\
         sig (Token n) = 5\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'n' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

// ---------------------------------------------------------------------------
// Strict GHC parity on scalars: a scalar derived from a `%1` value is held
// to exactly-once like every other alias — there is no scalar-memoization
// exemption. These programs were ACCEPTED under the old at-least-once
// scalar rule (duplication was considered free because the runtime
// memoizes the thunk); GHC rejects all of them, and so does mata-ll now.
// The legitimate exactly-once scalar shapes still compile — see useOnce /
// onceVia in linear_affine_basic.mll and viaMaybe in linear_mult_poly.mll.
// ---------------------------------------------------------------------------

/// The canonical scalar duplication: a where-binding built from a `%1`
/// value read twice. Operationally harmless under memoization, but GHC
/// has no scalar exemption — parity rejects it.
#[test]
fn linear_rejects_scalar_where_binding_double_use() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         bad :: Token %1 -> Int\n\
         bad t = go + go\n\
         \x20 where go = useOnce t\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local binding 'go'"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// The multi-step scalar launder that was the one known ACCEPT-direction
/// hole: the pending consumption of 't' sits in the thunk of the scalar
/// binding 'n', and the unrestricted 'constUnit' may never force it — the
/// leak used to slip through because scalar bindings were untracked.
#[test]
fn linear_rejects_scalar_laundered_through_let_binding() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         constUnit :: Int -> ()\n\
         constUnit x = ()\n\
         bad :: Token %1 -> ()\n\
         bad t = let n = useOnce t in constUnit n\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'t' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("local binding 'n'"), "{}", msg);
    assert!(msg.contains("constUnit"), "{}", msg);
}

/// The derived-alias form of the launder: a scalar pattern-bound from a
/// tainted match handed to an unrestricted function, which may drop (or
/// duplicate) it — its one obligated consumption may never happen.
#[test]
fn linear_rejects_scalar_alias_flow_into_unrestricted_function() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         step :: Token %1 -> (Token, Int)\n\
         step t = (t, 5)\n\
         constInt :: Int -> Int\n\
         constInt x = 7\n\
         bad :: Token %1 -> Int\n\
         bad t = case step t of\n\
         \x20 (t2, n) -> useOnce t2 + constInt n\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'n' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("pattern-bound from 't'"), "{}", msg);
    assert!(msg.contains("constInt"), "{}", msg);
}

/// A tracked scalar captured by a lambda: the closure may run any number
/// of times — or never, leaking the consumption parked in the scalar's
/// thunk. (Was charged ω but accepted under the old scalar rule; a
/// non-scalar capture was always rejected.)
#[test]
fn linear_rejects_scalar_captured_by_lambda() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         bad :: Token %1 -> (Int -> Int)\n\
         bad (Token n) = \\x -> n + x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'n' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("captured by a function value"), "{}", msg);
}

/// A `>>=` whose continuation is a NAMED function with an unrestricted
/// arrow: the bound value (an alias of the `%1` argument) flows somewhere
/// that promises neither exactly-once nor at-most-once. This was a
/// false-accept under the affine checker (only lambda continuations were
/// tracked).
#[test]
fn linear_rejects_unrestricted_bind_continuation() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useTwiceIO :: Token -> IO ()\n\
         useTwiceIO (Token n) = print (n + n)\n\
         unbox :: Token %1 -> Token\n\
         unbox t = t\n\
         f :: Token %1 -> IO ()\n\
         f b = pure (unbox b) >>= useTwiceIO\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'b' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("not '%1'"), "{}", msg);
}

/// A scrutinee that consumes two tracked values (the scalar field 'a' and
/// the `%1` value 't' — both exactly-once, scalars included) taints the
/// tuple's binders; a double use of the aliased 'tok' is a double use of
/// 't' and rejects. (The origin names 'a': among equal-rank sources the
/// taint picks the alphabetically first for stable diagnostics.)
#[test]
fn linear_rejects_double_use_through_multi_source_taint() {
    let msg = expect_linear_reject(
        "data Token = Token Int\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         g :: Token %1 -> Token %1 -> Int\n\
         g (Token a) t = case (a, t) of\n\
         \x20 (x, tok) -> useOnce tok + useOnce tok + x\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'tok' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("pattern-bound from 'a'"), "{}", msg);
}

/// Erasure of the exactly-once positive shapes: a tainted case consumed in
/// every branch, and a scalar alias forced once, emit byte-identical Lua
/// with and without the `%1` annotations.
#[test]
fn linear_exactly_once_erases_to_identical_lua() {
    let with_mult = "data Token = Token Int\n\
         data Box = Box Token\n\
         useOnce :: Token %1 -> Int\n\
         useOnce (Token n) = n\n\
         caseBoth :: Box %1 -> Int -> Int\n\
         caseBoth b n = case b of\n\
         \x20 Box t -> if n > 0 then useOnce t else useOnce t + 1\n\
         step :: Token %1 -> (Token, Int)\n\
         step t = (t, 5)\n\
         f :: Token %1 -> Int\n\
         f t = case step t of\n\
         \x20 (t2, n) -> useOnce t2 + n\n\
         main :: IO ()\n\
         main = do\n\
         \x20 print (caseBoth (Box (Token 4)) 1)\n\
         \x20 print (f (Token 37))\n";
    let without_mult = with_mult.replace("%1 ->", "->");
    let a = compile(with_mult, Path::new("tests/cases"), &[])
        .expect("the %1 program must compile")
        .lua_code;
    let b = compile(&without_mult, Path::new("tests/cases"), &[])
        .expect("the plain-arrow program must compile")
        .lua_code;
    assert!(a == b, "%1 must erase: emitted Lua differs");
}

// ---------------------------------------------------------------------------
// LIOLinear: the linear file-handle library's guarantee is the usage checker's
// — a WHandle crossing a `%1` arrow must be written/closed exactly once. These
// compile against the real library (lib/LIOLinear.mll), so they pin down the
// two misuses the API exists to prevent: forgetting hClose (a leaked file
// handle) and touching a handle after it has been consumed (write/close after
// close). The well-formed side runs in lib_liolinear.mll.
// ---------------------------------------------------------------------------

/// Like expect_linear_reject, but with the lib/ search path so the program
/// can import LIOLinear.
fn expect_linear_reject_with_lib(src: &str) -> String {
    let lib_path = Path::new("../lib");
    match compile(src, Path::new("tests/cases"), &[lib_path]) {
        Ok(_) => panic!(
            "this program violates the %1 (exactly-once) discipline and \
             must NOT compile:\n{}",
            src
        ),
        Err(e) => format!("{}", e),
    }
}

/// Forgetting hClose: the handle threaded out of hPut is never consumed, so
/// the underlying file would leak — rejected.
#[test]
fn linear_rejects_liolinear_forgotten_close() {
    let msg = expect_linear_reject_with_lib(
        "import LIOLinear (WHandle, hPut, hClose, withOutFile)\n\
         main :: IO ()\n\
         main = do\n\
         \x20 r <- withOutFile \"/tmp/mll-liolinear-leak\" (\\h -> do\n\
         \x20\x20 h2 <- hPut h \"hello\"\n\
         \x20\x20 putStrLn \"forgot to close\")\n\
         \x20 case r of\n\
         \x20\x20 Left err -> error err\n\
         \x20\x20 Right _ -> pure ()\n",
    );
    assert!(msg.contains("'h2' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("consumed zero times"), "{}", msg);
}

/// Using a handle after it has been consumed: the first hPut consumed `h`,
/// the second write is the double-close/double-free class of bug — rejected.
#[test]
fn linear_rejects_liolinear_use_after_consume() {
    let msg = expect_linear_reject_with_lib(
        "import LIOLinear (WHandle, hPut, hClose, withOutFile)\n\
         main :: IO ()\n\
         main = do\n\
         \x20 r <- withOutFile \"/tmp/mll-liolinear-twice\" (\\h -> do\n\
         \x20\x20 h2 <- hPut h \"first\"\n\
         \x20\x20 hClose h2\n\
         \x20\x20 h3 <- hPut h \"again\"\n\
         \x20\x20 hClose h3)\n\
         \x20 case r of\n\
         \x20\x20 Left err -> error err\n\
         \x20\x20 Right _ -> pure ()\n",
    );
    assert!(msg.contains("'h' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}

/// Double close through a %1-typed writer function (the openOut entry path):
/// the handle is linear for the whole body, so closing twice is two uses.
#[test]
fn linear_rejects_liolinear_double_close() {
    let msg = expect_linear_reject_with_lib(
        "import LIOLinear (WHandle, hPut, hClose)\n\
         closeTwice :: WHandle %1 -> IO ()\n\
         closeTwice h = do\n\
         \x20 hClose h\n\
         \x20 hClose h\n\
         main :: IO ()\n\
         main = putStrLn \"no\"\n",
    );
    assert!(msg.contains("'h' must be consumed exactly once"), "{}", msg);
    assert!(msg.contains("more than once"), "{}", msg);
}
