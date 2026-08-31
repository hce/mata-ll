-- A15: monadic binds over general patterns. `()`, constructor patterns,
-- nested tuples, list-cons patterns — all GHC-legal do-binds that used to
-- be parse errors ("do-block <- binds a variable, _, or a tuple only").
-- A refutable pattern desugars with GHC's MonadFail-style fallback (see
-- do_pattern_bind_failure.mll for the failing side); irrefutable patterns
-- desugar exactly as the old tuple-only path did.

first3 :: [Int] -> IO Int
first3 xs = do
    (a : b : c : _) <- return xs
    return (a + b + c)

main :: IO ()
main = do
    () <- return ()
    Just x <- return (Just 5)
    (a, b) <- return (1, 2)
    (p, (q, r)) <- return (10, (20, 30))
    Right v <- return (Right 7 :: Either String Int)
    s <- first3 [100, 200, 300]
    print (x + a + b)
    print (p + q + r)
    print v
    print s
