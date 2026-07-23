-- Performance test suite for non-strict semantics with cheapness analysis.
--
-- Non-strict evaluation: function arguments are thunked unless "cheap"
-- (arithmetic, variables, literals, constructors). This prevents thunk
-- chain buildup in accumulator patterns while preserving laziness for
-- expensive computations.
--
-- Run: mll examples/perf_laziness.mll && lua examples/perf_laziness.lua

clockRaw :: Int -> LuaIO "os.clock" Number

clock :: IO Number
clock = clockRaw 0

-- Tight accumulator loop — args are cheap (arithmetic), not thunked.
sumStrict :: Int -> Int
sumStrict n = go 0 0
    where
        go acc i
            | i > n     = acc
            | otherwise = go (acc + i) (i + 1)

buildList :: Int -> [Int]
buildList n = go n []
    where
        go 0 acc = acc
        go i acc = go (i - 1) (i : acc)

sumList :: [Int] -> Int
sumList xs = foldl (+) 0 xs

fibs :: [Int]
fibs = 0 : 1 : zipWith (+) fibs (tail fibs)

nats :: [Int]
nats = 1 : map (+1) nats

deepPipeline :: [Int] -> Int
deepPipeline xs = foldl (+) 0 (filter (\x -> x > 100) (map (\x -> x + 1) (filter (\x -> x > 10) (map (*3) (map (*2) xs)))))

expensiveId :: Int -> Int
expensiveId n = sumStrict n

-- Non-strict wins: unused arg is not evaluated.
conditionalUse :: Bool -> Int -> Int
conditionalUse True _ = 42
conditionalUse False x = x

sharedBinding :: Int -> Int
sharedBinding n = let expensive = sumStrict n in expensive + expensive

deadBinding :: Int -> Int
deadBinding n = let unused = sumStrict n in 42

data Triple = MkTriple Int Int Int

sumTripleField :: Triple -> Int
sumTripleField (MkTriple a b c) = a + b + c

buildTriples :: Int -> Int
buildTriples n = go 0 0
    where
        go acc i
            | i >= n    = acc
            | otherwise = go (acc + sumTripleField (MkTriple i (i+1) (i+2))) (i + 1)

fib :: Int -> Int
fib 0 = 0
fib 1 = 1
fib n = fib (n - 1) + fib (n - 2)

bench1 :: IO ()
bench1 = do
    putStrLn "-- 1. Accumulator loop (should NOT thunk) --"
    t0 <- clock
    let r = sumStrict 500000
    putStrLn ("    sumStrict 500000 = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench2 :: IO ()
bench2 = do
    putStrLn "-- 2. List sum via foldl --"
    t0 <- clock
    let r = sumList (buildList 50000)
    putStrLn ("    sumList 50000 = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench3 :: IO ()
bench3 = do
    putStrLn "-- 3. Infinite structures (lazy cons) --"
    t0 <- clock
    let r = foldl (+) 0 (take 1000 nats)
    putStrLn ("    sum(take 1000 nats) = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench3b :: IO ()
bench3b = do
    t0 <- clock
    let r = head (reverse (take 31 fibs))
    putStrLn ("    fib(30) lazy list = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench4 :: IO ()
bench4 = do
    putStrLn "-- 4. Pipeline (map/filter chain) --"
    t0 <- clock
    let r = deepPipeline (buildList 50000)
    putStrLn ("    deepPipeline 50000 = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench5 :: IO ()
bench5 = do
    putStrLn "-- 5. Conditional eval (non-strict wins here) --"
    t0 <- clock
    let r = conditionalUse True (expensiveId 500000)
    putStrLn ("    True branch (skip expensive) = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench5b :: IO ()
bench5b = do
    t0 <- clock
    let r = conditionalUse False (expensiveId 500000)
    putStrLn ("    False branch (force expensive) = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench6 :: IO ()
bench6 = do
    putStrLn "-- 6. Dead binding (non-strict wins here) --"
    t0 <- clock
    let r = deadBinding 500000
    putStrLn ("    deadBinding 500000 = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench7 :: IO ()
bench7 = do
    putStrLn "-- 7. ADT construction --"
    t0 <- clock
    let r = buildTriples 100000
    putStrLn ("    buildTriples 100000 = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

bench8 :: IO ()
bench8 = do
    putStrLn "-- 8. Recursive fib (baseline) --"
    t0 <- clock
    let r = fib 30
    putStrLn ("    fib 30 = " <> show r)
    t1 <- clock
    putStrLn ("  time: " <> show (t1 - t0) <> "s")

main :: IO ()
main = do
    putStrLn "=== Non-Strict Evaluation Benchmark ==="
    putStrLn ""
    bench1
    bench2
    bench3
    bench3b
    bench4
    bench5
    bench5b
    bench6
    bench7
    bench8
    putStrLn ""
    putStrLn "=== Done ==="
