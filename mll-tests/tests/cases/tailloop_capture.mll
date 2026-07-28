-- Self-tail-call → loop conversion (opt pass 5): closures created in one
-- iteration must keep seeing THAT iteration's values after later
-- iterations run — the per-call-fresh-locals property of recursion. The
-- loop conversion keeps the carried state in the real parameters but gives
-- every iteration fresh working locals, so a captured value is the
-- iteration's own. If the conversion let closures capture the mutated
-- parameters instead, every closure here would see the final counter.

-- Lazily accumulated list: each `n * 10` is suspended (a closure over the
-- iteration's binding) and only forced after the whole loop has finished.
buildDown :: Int -> [Int] -> [Int]
buildDown 0 acc = acc
buildDown n acc = buildDown (n - 1) (n * 10 : acc)

-- First-class functions accumulated across iterations: each lambda
-- captures its iteration's n, and all are applied after the loop exits.
mkAdders :: Int -> [Int -> Int] -> [Int -> Int]
mkAdders 0 acc = acc
mkAdders n acc = mkAdders (n - 1) ((\x -> x + n) : acc)

main :: IO ()
main = do
    assert (buildDown 4 [] == [10, 20, 30, 40]) "suspended per-iteration values"
    assert (map (\f -> f 0) (mkAdders 4 []) == [1, 2, 3, 4]) "captured per-iteration lambdas"
    assert (map (\f -> f 100) (mkAdders 2 []) == [101, 102]) "captures survive re-run"
