-- ghc_regr018: do-notation: let + bind + when + mapM_ mixed

sumList :: [Integer] -> Integer
sumList []     = 0
sumList (x:xs) = x + sumList xs

double :: Integer -> Integer
double n = n * 2

square :: Integer -> Integer
square n = n * n

addOne :: Integer -> Integer
addOne n = n + 1

main :: IO ()
main = do
    -- Basic let in do
    let a = 42 :: Integer
    assert (a == 42) "basic let"

    -- Multiple chained lets
    let b = 10 :: Integer
    let c = b + 5
    assert (c == 15) "let chain"

    -- let with function call
    let d = double b
    assert (d == 20) "let fn call"

    -- when: condition true executes action
    let flag = True
    when flag (assert True "when true branch")

    -- when: false condition (no observable IO action)
    when False (return ())

    -- mapM_ over non-empty list
    let xs = [1 :: Integer, 2, 3]
    mapM_ (\x -> assert (x > 0) "positive") xs

    -- mapM_ over empty list (nothing should happen)
    mapM_ (\_ -> return ()) ([] :: [Integer])

    -- let with computed value
    let n = 5 :: Integer
    let sq = square n
    assert (sq == 25) "square"

    -- when with computed condition
    let total = sumList [1 :: Integer, 2, 3, 4, 5]
    assert (total == 15) "sumList"
    when (total > 10) (assert True "total gt 10")
    when (total < 0) (return ())

    -- seq: force evaluation order
    let forced = seq (1 + 1 :: Integer) True
    assert forced "seq forces"

    -- mapM_ with assertion on each element
    let ys = [10 :: Integer, 20, 30]
    mapM_ (\y -> assert (y >= 10) "ys element") ys

    -- nested when
    when True (when True (assert True "nested when"))

    -- mapM_ with multiple operations
    let zs = [1 :: Integer, 2, 3, 4, 5]
    mapM_ (\z -> assert (z * z > 0) "z squared") zs

    putStrLn "ok"
