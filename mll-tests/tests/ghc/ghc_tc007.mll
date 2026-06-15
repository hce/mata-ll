-- GHC tc007: Type inference with numeric literals
-- Tests that numeric literals unify correctly with Integer and Number

addI :: Integer -> Integer -> Integer
addI x y = x + y

addN :: Number -> Number -> Number
addN x y = x + y

-- Polymorphic numeric operations
clamp :: Integer -> Integer -> Integer -> Integer
clamp lo hi x
    | x < lo    = lo
    | x > hi    = hi
    | otherwise = x

average :: Number -> Number -> Number
average a b = (a + b) / 2.0

factorial :: Integer -> Integer
factorial n
    | n <= 0    = 1
    | otherwise = n * factorial (n - 1)

sumList :: [Integer] -> Integer
sumList xs = foldl (+) 0 xs

productList :: [Integer] -> Integer
productList xs = foldl (*) 1 xs

main :: IO ()
main = do
    -- Integer literal inference
    assert (addI 3 4 == 7) "addI"
    assert (addI 100 200 == 300) "addI large"
    assert (addI 0 0 == 0) "addI zero"

    -- Number literal inference
    assert (addN 1.5 2.5 == 4.0) "addN"
    assert (average 3.0 7.0 == 5.0) "average"

    -- Clamp with literal bounds
    assert (clamp 0 10 5 == 5) "clamp mid"
    assert (clamp 0 10 15 == 10) "clamp hi"
    assert (clamp 0 10 (0 - 3) == 0) "clamp lo"

    -- Factorial (recursive numeric)
    assert (factorial 0 == 1) "fact 0"
    assert (factorial 5 == 120) "fact 5"
    assert (factorial 10 == 3628800) "fact 10"

    -- List numeric operations
    assert (sumList [1, 2, 3, 4, 5] == 15) "sumList"
    assert (productList [1, 2, 3, 4, 5] == 120) "productList"
    assert (sumList [] == 0) "sumList empty"

    putStrLn "ok"
