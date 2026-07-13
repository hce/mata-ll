-- Regression (Finding 3, part 1 — APPLIES TO ALL TARGETS):
-- `div` by zero must RAISE a runtime error, exactly like GHC's
-- "divide by zero". Today `a \`div\` b` compiles to `math.floor(a / b)`,
-- which is FLOAT division: `1 \`div\` 0` silently yields `inf` (a float
-- posing as an Integer) and `0 \`div\` 0` yields nan. `mod` by zero
-- already raises on the Lua 5.4 target (integer `%` traps on a zero
-- divisor) — that behaviour is pinned here too so it never regresses.
--
-- All divisions go through opaque top-level functions (variable
-- operands), so constant folding (fold.rs) cannot evaluate them at
-- compile time and hide the runtime behaviour. The bare-literal infix
-- form is tested as well: fold.rs deliberately refuses to fold a zero
-- divisor, so the literal form also reaches the runtime path.
--
-- Idiom: `try` + `pure`, as in exceptions.mll (test 7 there proves the
-- pure result is forced inside the try, so the error is catchable).

dz :: Integer -> Integer -> Integer
dz a b = a `div` b

mz :: Integer -> Integer -> Integer
mz a b = a `mod` b

main :: IO ()
main = do
    -- 1 `div` 0 through a function: must raise, not return inf.
    r1 <- try (pure (dz 1 0))
    case r1 of
        Right v -> error ("dz 1 0 must raise, but returned: " <> show v)
        Left _  -> putStrLn "div by zero raises (function form)"

    -- 0 `div` 0: must raise, not return nan.
    r2 <- try (pure (dz 0 0))
    case r2 of
        Right v -> error ("dz 0 0 must raise, but returned: " <> show v)
        Left _  -> putStrLn "0 div 0 raises"

    -- Negative dividend: must raise, not return -inf.
    r3 <- try (pure (dz (-9) 0))
    case r3 of
        Right v -> error ("dz (-9) 0 must raise, but returned: " <> show v)
        Left _  -> putStrLn "negative div by zero raises"

    -- mod by zero (function form): already raises today; keep it that way.
    r4 <- try (pure (mz 5 0))
    case r4 of
        Right v -> error ("mz 5 0 must raise, but returned: " <> show v)
        Left _  -> putStrLn "mod by zero raises (function form)"

    r5 <- try (pure (mz 0 0))
    case r5 of
        Right v -> error ("mz 0 0 must raise, but returned: " <> show v)
        Left _  -> putStrLn "0 mod 0 raises"

    -- Bare infix literal forms (unfolded: fold.rs skips zero divisors).
    r6 <- try (pure ((1 :: Integer) `div` 0))
    case r6 of
        Right v -> error ("1 `div` 0 must raise, but returned: " <> show v)
        Left _  -> putStrLn "div by zero raises (literal infix form)"

    r7 <- try (pure ((5 :: Integer) `mod` 0))
    case r7 of
        Right v -> error ("5 `mod` 0 must raise, but returned: " <> show v)
        Left _  -> putStrLn "mod by zero raises (literal infix form)"

    -- Divisor arriving through a binding, not a literal — the shape the
    -- optimizer is most tempted to pre-evaluate or specialize.
    let zero = 0 :: Integer
    r8 <- try (pure (10 `div` zero))
    case r8 of
        Right v -> error ("10 `div` zero must raise, but returned: " <> show v)
        Left _  -> putStrLn "div by let-bound zero raises"

    -- Zero produced by arithmetic at runtime.
    r9 <- try (pure (dz 100 (5 - 5)))
    case r9 of
        Right v -> error ("dz 100 (5-5) must raise, but returned: " <> show v)
        Left _  -> putStrLn "div by computed zero raises"

    putStrLn "ok"
