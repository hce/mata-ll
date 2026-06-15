-- ghc_regr002: Recursive function with accumulator and strict evaluation (seq)

-- Strict left fold using seq to force the accumulator
foldlStrict :: (b -> a -> b) -> b -> [a] -> b
foldlStrict f acc []     = acc
foldlStrict f acc (x:xs) = seq (f acc x) (foldlStrict f (f acc x) xs)

-- Sum using strict accumulator
sumStrict :: [Integer] -> Integer
sumStrict xs = foldlStrict (+) 0 xs

-- Product using strict accumulator
productStrict :: [Integer] -> Integer
productStrict xs = foldlStrict (*) 1 xs

-- Max via strict fold
maxStrict :: Integer -> [Integer] -> Integer
maxStrict z xs = foldlStrict (\a b -> if a > b then a else b) z xs

-- Strict fibonacci via accumulator (iterative)
fibStep :: Integer -> Integer -> Integer -> Integer
fibStep n a b
    | n == 0    = a
    | otherwise = seq b (fibStep (n - 1) b (a + b))

fib :: Integer -> Integer
fib n = fibStep n 0 1

-- Strict length via accumulator
lengthStrict :: [a] -> Integer
lengthStrict []     = 0
lengthStrict (x:xs) = seq x (1 + lengthStrict xs)

main :: IO ()
main = do
    assert (sumStrict [1..100] == 5050) "sum 1..100"
    assert (sumStrict [] == 0) "sum empty"
    assert (productStrict [1..5] == 120) "product 1..5"
    assert (maxStrict 0 [3, 1, 4, 1, 5, 9, 2, 6] == 9) "max"
    assert (fib 0 == 0) "fib 0"
    assert (fib 1 == 1) "fib 1"
    assert (fib 10 == 55) "fib 10"
    assert (fib 20 == 6765) "fib 20"
    assert (lengthStrict [1..50] == 50) "length 50"
    assert (seq 42 True == True) "seq returns second"
    assert (seq "hello" 99 == 99) "seq string"
    putStrLn "ok"
