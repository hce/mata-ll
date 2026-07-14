-- Regression (Finding 3, part 1 — APPLIES TO ALL TARGETS):
-- `div` by zero must RAISE a runtime error, exactly like GHC's
-- "divide by zero". `a `div` b` goes through __mll_div, which raises on a
-- zero divisor on every host (it used to compile to `math.floor(a / b)`,
-- FLOAT division: `1 `div` 0` silently yielded `inf`, a float posing as an
-- Integer, and `0 `div` 0` yielded nan). `mod` by zero raises the same way
-- through __mll_mod.
--
-- All divisions go through opaque top-level functions (variable operands),
-- so constant folding (fold.rs) cannot evaluate them at compile time and
-- hide the runtime behaviour. The bare-literal infix form is tested as well:
-- fold.rs deliberately refuses to fold a zero divisor, so the literal form
-- also reaches the runtime path.
--
-- Idiom: `try (X `seq` pure ())`, matching div_exact_and_zero.mll. `seq`
-- DEMANDS the quotient to WHNF *inside* the tried action, so the divide-by-
-- zero error is raised (and caught) there — the Haskell-correct way to force
-- a pure value inside `try`. This does NOT depend on how eagerly `pure`
-- evaluates: `pure` is non-strict (a `return ⊥` is inert until demanded, per
-- SPEC's eagerness contract), so `try (pure (dz 1 0))` would return
-- `Right <thunk>` and the error would escape the `try` — exactly as in GHC.

dz :: Integer -> Integer -> Integer
dz a b = a `div` b

mz :: Integer -> Integer -> Integer
mz a b = a `mod` b

main :: IO ()
main = do
    -- 1 `div` 0 through a function: must raise, not return inf.
    r1 <- try (dz 1 0 `seq` pure ())
    case r1 of
        Right () -> error "dz 1 0 must raise, but returned normally"
        Left _   -> putStrLn "div by zero raises (function form)"

    -- 0 `div` 0: must raise, not return nan.
    r2 <- try (dz 0 0 `seq` pure ())
    case r2 of
        Right () -> error "dz 0 0 must raise, but returned normally"
        Left _   -> putStrLn "0 div 0 raises"

    -- Negative dividend: must raise, not return -inf.
    r3 <- try (dz (-9) 0 `seq` pure ())
    case r3 of
        Right () -> error "dz (-9) 0 must raise, but returned normally"
        Left _   -> putStrLn "negative div by zero raises"

    -- mod by zero (function form): must raise.
    r4 <- try (mz 5 0 `seq` pure ())
    case r4 of
        Right () -> error "mz 5 0 must raise, but returned normally"
        Left _   -> putStrLn "mod by zero raises (function form)"

    r5 <- try (mz 0 0 `seq` pure ())
    case r5 of
        Right () -> error "mz 0 0 must raise, but returned normally"
        Left _   -> putStrLn "0 mod 0 raises"

    -- Bare infix literal forms (unfolded: fold.rs skips zero divisors).
    r6 <- try (((1 :: Integer) `div` 0) `seq` pure ())
    case r6 of
        Right () -> error "1 `div` 0 must raise, but returned normally"
        Left _   -> putStrLn "div by zero raises (literal infix form)"

    r7 <- try (((5 :: Integer) `mod` 0) `seq` pure ())
    case r7 of
        Right () -> error "5 `mod` 0 must raise, but returned normally"
        Left _   -> putStrLn "mod by zero raises (literal infix form)"

    -- Divisor arriving through a binding, not a literal — the shape the
    -- optimizer is most tempted to pre-evaluate or specialize.
    let zero = 0 :: Integer
    r8 <- try ((10 `div` zero) `seq` pure ())
    case r8 of
        Right () -> error "10 `div` zero must raise, but returned normally"
        Left _   -> putStrLn "div by let-bound zero raises"

    -- Zero produced by arithmetic at runtime.
    r9 <- try (dz 100 (5 - 5) `seq` pure ())
    case r9 of
        Right () -> error "dz 100 (5-5) must raise, but returned normally"
        Left _   -> putStrLn "div by computed zero raises"

    putStrLn "ok"
