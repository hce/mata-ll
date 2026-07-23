-- GHC cgrun036: CPS (continuation-passing style)
-- Tests higher-order functions with continuations

-- CPS factorial
factCPS :: Int -> (Int -> a) -> a
factCPS 0 k = k 1
factCPS n k = factCPS (n - 1) (\r -> k (n * r))

-- CPS fibonacci
fibCPS :: Int -> (Int -> a) -> a
fibCPS 0 k = k 0
fibCPS 1 k = k 1
fibCPS n k = fibCPS (n - 1) (\a -> fibCPS (n - 2) (\b -> k (a + b)))

-- CPS list sum
sumCPS :: [Int] -> (Int -> a) -> a
sumCPS [] k     = k 0
sumCPS (x:xs) k = sumCPS xs (\s -> k (x + s))

main :: IO ()
main = do
    assert (factCPS 0 id == 1) "cps fact 0"
    assert (factCPS 5 id == 120) "cps fact 5"
    assert (factCPS 10 id == 3628800) "cps fact 10"

    assert (fibCPS 0 id == 0) "cps fib 0"
    assert (fibCPS 1 id == 1) "cps fib 1"
    assert (fibCPS 10 id == 55) "cps fib 10"

    assert (sumCPS [1, 2, 3, 4, 5] id == 15) "cps sum"
    assert (sumCPS ([] :: [Int]) id == 0) "cps sum empty"

    -- CPS with non-identity continuation
    assert (factCPS 5 (* 2) == 240) "cps fact cont"
    assert (factCPS 5 show == "120") "cps fact show"

    putStrLn "ok"
