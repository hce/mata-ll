-- Performance benchmark for MATA-LL
-- Measures time for various operations to detect regressions.

clockRaw :: Integer -> LuaIO "os.clock" Number

clock :: IO Number
clock = clockRaw 0

-- Arithmetic-heavy: sum 1 to n
sumTo :: Integer -> Integer
sumTo n = go 0 0
    where
        go acc i
            | i > n     = acc
            | otherwise = go (acc + i) (i + 1)

-- List creation: build a list of n elements
buildList :: Integer -> [Integer]
buildList n = go n []
    where
        go 0 acc = acc
        go i acc = go (i - 1) (i : acc)

-- List traversal: sum all elements
sumList :: [Integer] -> Integer
sumList [] = 0
sumList (x:xs) = x + sumList xs

-- Map + filter + fold pipeline
pipeline :: [Integer] -> Integer
pipeline xs = foldl (+) 0 (filter (\x -> x > 50) (map (*2) xs))

-- Recursive fibonacci (exponential, stress test)
fib :: Integer -> Integer
fib 0 = 0
fib 1 = 1
fib n = fib (n - 1) + fib (n - 2)

-- Pattern matching heavy: classify many values
data Color = Red | Green | Blue

colorVal :: Color -> Integer
colorVal Red = 1
colorVal Green = 2
colorVal Blue = 3

colorBench :: Integer -> Integer
colorBench n = go 0 0
    where
        go acc i
            | i >= n    = acc
            | otherwise = go (acc + colorVal Red + colorVal Green + colorVal Blue) (i + 1)

-- String concatenation
buildString :: Integer -> String
buildString 0 = ""
buildString n = "x" <> buildString (n - 1)

-- Higher-order function overhead
applyN :: Integer -> (a -> a) -> a -> a
applyN 0 _ x = x
applyN n f x = applyN (n - 1) f (f x)

-- Benchmark runner
bench :: String -> IO () -> IO ()
bench name action = do
    t1 <- clock
    action
    t2 <- clock
    putStrLn (name <> ": " <> show (t2 - t1) <> "s")

main :: IO ()
main = do
    putStrLn "=== MATA-LL Benchmark ==="

    bench "sumTo 1000000" (do
        let r = sumTo 1000000
        putStrLn ("  result: " <> show r))

    bench "buildList 10000 + sumList" (do
        let xs = buildList 10000
        let r = sumList xs
        putStrLn ("  result: " <> show r))

    bench "pipeline (10000 elements)" (do
        let xs = buildList 10000
        let r = pipeline xs
        putStrLn ("  result: " <> show r))

    bench "fib 30" (do
        let r = fib 30
        putStrLn ("  result: " <> show r))

    bench "colorBench 100000" (do
        let r = colorBench 100000
        putStrLn ("  result: " <> show r))

    bench "buildString 10000" (do
        let r = length (buildString 10000)
        putStrLn ("  length: " <> show r))

    bench "applyN 1000000 (+1) 0" (do
        let r = applyN 1000000 (+1) 0
        putStrLn ("  result: " <> show r))

    putStrLn "=== Done ==="
