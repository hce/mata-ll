-- GHC ds014: Negative literal patterns and expressions

listIndex :: [a] -> Int -> a
listIndex (x:_)  0 = x
listIndex (_:xs) n = listIndex xs (n - 1)
listIndex []     _ = error "index out of bounds"

-- Guards with negative values
signum' :: Int -> Int
signum' n
    | n < 0     = 0 - 1
    | n == 0    = 0
    | otherwise = 1

-- Arithmetic with negatives
negArith :: Int -> Int -> Int
negArith x y = (0 - x) * y + (0 - y)

-- Negative in list
negList :: [Int]
negList = [0 - 5, 0 - 4, 0 - 3, 0 - 2, 0 - 1, 0, 1, 2, 3, 4, 5]

-- filter negative
negatives :: [Int] -> [Int]
negatives xs = filter (< 0) xs

-- absolute value
myAbs :: Int -> Int
myAbs n
    | n < 0     = 0 - n
    | otherwise = n

-- Case on negative result
checkNeg :: Int -> String
checkNeg n = case signum' n of
    1          -> "positive"
    0          -> "zero"
    _          -> "negative"

main :: IO ()
main = do
    -- Negative arithmetic
    assert (0 - 5 + 3 == 0 - 2) "neg arith"
    assert (0 - 3 * 2 == 0 - 6) "neg mul"
    assert ((0 - 4) `div` 2 == 0 - 2) "neg div"

    -- Guards with negatives
    assert (signum' (0 - 5) == 0 - 1) "signum neg"
    assert (signum' 0 == 0)  "signum zero"
    assert (signum' 3 == 1)  "signum pos"

    -- negArith
    assert (negArith 2 3 == (0 - 2) * 3 + (0 - 3)) "negArith"

    -- negList properties
    assert (length negList == 11) "negList length"
    assert (listIndex negList 0 == 0 - 5) "negList head"
    assert (listIndex negList 10 == 5)  "negList tail"

    -- filter negatives
    assert (negatives negList == [0-5, 0-4, 0-3, 0-2, 0-1]) "negatives"
    assert (negatives [1,2,3] == []) "no negatives"

    -- abs
    assert (myAbs (0 - 7) == 7) "abs neg"
    assert (myAbs 0  == 0)  "abs zero"
    assert (myAbs 4  == 4)  "abs pos"

    -- case on negative
    assert (checkNeg (0 - 1) == "negative") "check neg"
    assert (checkNeg 0  == "zero")    "check zero"
    assert (checkNeg 5  == "positive") "check pos"

    putStrLn "ok"
