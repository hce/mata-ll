-- A14 (the remaining half — `let f x = e` has worked for a while): PATTERN
-- parameters in local bindings, desugared exactly like pattern lambdas
-- (fresh parameter + case, with GHC's partial-function fallback for a
-- refutable pattern). Covers let-in, do-let, and where bindings.

topDist :: (Int, Int) -> Int
topDist p = go p
  where
    go (a, b) = a * a + b * b

main :: IO ()
main = do
    let dist (a, b) = a + b
    print (dist (3, 4))
    print (let f (x, (y, z)) = x * y * z in f (2, (3, 4)))
    print (topDist (3, 4))
    -- A refutable pattern parameter is accepted with GHC's partial-function
    -- semantics; the matching call succeeds here, and the failing side is
    -- pinned in do_pattern_bind_failure.mll (excluded from the oracle: the
    -- message formats differ).
    let fromJust' (Just v) = v
    print (fromJust' (Just 41) + 1)

-- expect: 7
-- expect: 24
-- expect: 25
-- expect: 42
