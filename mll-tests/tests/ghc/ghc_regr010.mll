-- ghc_regr010: Lazy evaluation: infinite list operations (iterate)

-- iterate: produces infinite list
myIterate :: (a -> a) -> a -> [a]
myIterate f x = x : myIterate f (f x)

takeN :: Integer -> [a] -> [a]
takeN 0 _      = []
takeN _ []     = []
takeN n (x:xs) = x : takeN (n - 1) xs

dropN :: Integer -> [a] -> [a]
dropN 0 xs     = xs
dropN _ []     = []
dropN n (_:xs) = dropN (n - 1) xs

-- naturals: 0, 1, 2, ...
naturals :: [Integer]
naturals = myIterate (\n -> n + 1) 0

-- powers of 2: 1, 2, 4, 8, ...
powersOf2 :: [Integer]
powersOf2 = myIterate (\n -> n * 2) 1

-- Fibonacci via zip with tail of self
fibs :: [Integer]
fibs = 0 : 1 : zipWith (+) fibs (tail fibs)

-- Collatz sequence (not infinite but uses lazy style)
collatz :: Integer -> [Integer]
collatz n
    | n == 1         = [1]
    | n `mod` 2 == 0 = n : collatz (n `div` 2)
    | otherwise      = n : collatz (3 * n + 1)

main :: IO ()
main = do
    -- naturals
    assert (takeN 5 naturals == [0, 1, 2, 3, 4]) "naturals 5"
    assert (head (dropN 100 naturals) == 100) "naturals drop 100"

    -- powers of 2
    assert (takeN 8 powersOf2 == [1, 2, 4, 8, 16, 32, 64, 128]) "powers of 2"
    assert (head (dropN 10 powersOf2) == 1024) "2^10"

    -- iterate with string growth
    let strs = myIterate (\s -> s ++ "x") "a"
    assert (head strs == "a") "iter str first"
    assert (head (dropN 3 strs) == "axxx") "iter str 4th"

    -- fibs
    assert (takeN 8 fibs == [0, 1, 1, 2, 3, 5, 8, 13]) "fibs 8"
    assert (head (dropN 10 fibs) == 55) "fib 10"

    -- collatz
    assert (collatz 6 == [6, 3, 10, 5, 16, 8, 4, 2, 1]) "collatz 6"
    assert (length (collatz 27) == 112) "collatz 27 length"

    -- lazy: filter on naturals
    let evens = filter (\n -> n `mod` 2 == 0) naturals
    assert (takeN 5 evens == [0, 2, 4, 6, 8]) "evens"

    putStrLn "ok"
