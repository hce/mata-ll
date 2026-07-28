-- IO self-loop conversion (opt pass 6): closures and thunks built in one
-- iteration must keep seeing THAT iteration's values after later
-- iterations run — the per-call-fresh-locals property of recursion, kept
-- by the loop's per-iteration working copies. Same probes as
-- tailloop_capture, but through IO self-loops (per-iteration effects
-- included, so the branch action closures are exercised too).

buildDown :: Int -> [Int] -> IO [Int]
buildDown 0 acc = pure acc
buildDown n acc = do
    when (n == 2) (putStrLn ("passing " <> show n))
    buildDown (n - 1) (n * 10 : acc)

mkAdders :: Int -> [Int -> Int] -> IO [Int -> Int]
mkAdders 0 acc = pure acc
mkAdders n acc = mkAdders (n - 1) ((\x -> x + n) : acc)

main :: IO ()
main = do
    xs <- buildDown 4 []
    assert (xs == [10, 20, 30, 40]) "suspended per-iteration values"
    fs <- mkAdders 4 []
    assert (map (\f -> f 0) fs == [1, 2, 3, 4]) "captured per-iteration lambdas"
    gs <- mkAdders 2 []
    assert (map (\f -> f 100) gs == [101, 102]) "captures survive re-run"
