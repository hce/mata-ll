-- Regression: `return`/`pure` must stay NON-STRICT ACROSS a function boundary,
-- not just inside the do-block that writes it. `return_non_strict.mll` covers the
-- intraprocedural and zero-arg forms; those already worked because a zero-arg
-- action compiles to a deferred closure that `__mll_run` CALLS. The gap this
-- pins is an APPLIED (argument-taking) user function whose terminal action is
-- `pure e`: it compiles to a value-form action returned directly, and the bind
-- `v <- mk 1` used to FORCE it (via `__mll_run`) even when `v` is never used,
-- raising where GHC does not. The fix tags such an escaped pure value so the
-- runner unwraps it without forcing or calling it.

boom :: Integer
boom = error "boom: interprocedural return forced its argument"

-- Applied function (takes n) whose terminal action is a possibly-⊥ pure value.
mk :: Integer -> IO Integer
mk n = do
    _ <- return ()
    pure (boom + n)

-- Applied function returning a FUNCTION value via pure. The old runtime, unable
-- to tell a value-action from an action closure, CALLED this with no arguments
-- ("arithmetic on a nil value"); the tag makes it a value, delivered intact.
mkFn :: Integer -> IO (Integer -> Integer)
mkFn k = do
    _ <- return ()
    pure (\x -> x + k)

-- Effectful accumulator whose terminal `pure (acc + row)` escapes to the caller.
step :: Integer -> Integer -> IO Integer
step row acc = do
    _ <- return ()
    pure (acc + row)

main :: IO ()
main = do
    -- 1. The core bug: an applied `pure ⊥` bound but never demanded must NOT
    --    raise. Before the fix this raised "boom" at the bind.
    v <- mk 1
    putStrLn "1: interprocedural `v <- mk 1` bound a bottom without forcing"

    -- 2. Applied pure-of-function is delivered as a value and applied normally.
    g <- mkFn 5
    assert (g 10 == 15) "2: pure-returned function applied, not spuriously called"

    -- 3. A demanded interprocedural bottom STILL raises when forced — laziness,
    --    not error-swallowing. `seq` forces `v` inside the tried action.
    r <- try (v `seq` pure ())
    case r of
        Right () -> error "3: forcing an interprocedural returned bottom must raise"
        Left _   -> putStrLn "3: demanding it raises when forced"

    -- 4. A total interprocedural pure threads its real value through the bind.
    s1 <- step 30 0
    s2 <- step 20 s1
    s3 <- step 10 s2
    assert (s3 == 60) "4: total interprocedural pure threads its value"

    putStrLn "interprocedural return/pure: non-strict, value-preserving, still-raising-when-demanded"
