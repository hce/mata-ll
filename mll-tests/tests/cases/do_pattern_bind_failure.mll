-- A15, the failing side: a refutable do-bind pattern that does not match
-- calls IO's `fail` (MonadFail), which raises GHC's IOException text with
-- the pattern's span. Catchable with `try` like any other `error`.
-- (Excluded from the GHC oracle: GHC's span names the source FILE, which
-- is the twin's temporary path; the harness compiles this case with no
-- file name, so the span is bare. The partial-function message below is
-- mata-ll's error string too.)

failing :: IO Int
failing = do
    Just x <- return (Nothing :: Maybe Int)
    return x

main :: IO ()
main = do
    r <- try failing
    case r of
        Left msg -> putStrLn msg
        Right v -> print v
    -- An irrefutable pattern has no fallback arm and cannot fail.
    (a, b) <- return (1, 2)
    print (a + b)
    -- The A14 counterpart: a refutable PATTERN PARAMETER's fallback (GHC's
    -- partial-function semantics). `try (return e)` would not catch a pure
    -- bottom — the eagerness contract, see CAVEATS — so the mismatch is
    -- forced inside the action.
    r2 <- try (print (let g (Just v) = v in g (Nothing :: Maybe Int)))
    case r2 of
        Left msg -> putStrLn msg
        Right _ -> putStrLn "unreachable"

-- expect: user error (Pattern match failure in 'do' block at 11:5-10)
-- expect: 3
-- expect: Non-exhaustive patterns in g
