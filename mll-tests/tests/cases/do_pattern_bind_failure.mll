-- A15, the failing side: a refutable do-bind pattern that does not match
-- raises GHC's MonadFail-style error, located at the bind. Catchable with
-- `try` like any other `error`. (Excluded from the GHC oracle: the message
-- is mata-ll's error string, where GHC renders an IOException with a
-- file-qualified span — same semantics, different formatting.)

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

-- expect: Pattern match failure in do expression at 9:5
-- expect: 3
-- expect: Non-exhaustive patterns in g
