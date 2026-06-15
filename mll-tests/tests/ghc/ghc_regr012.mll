-- ghc_regr012: Negative numbers in patterns and expressions

-- Negative integer expressions and guards
sign :: Integer -> String
sign n
    | n == 0    = "zero"
    | n == 1    = "one"
    | n == (-1) = "neg-one"
    | n > 0     = "positive"
    | otherwise = "negative"

-- Arithmetic with negatives
negArith :: Integer -> Integer -> Integer -> Integer
negArith a b c = a * (-1) + b - (-c)

-- Negative in list patterns
headNeg :: [Integer] -> Integer
headNeg []    = 0
headNeg (x:_) = x

-- Guards with negative thresholds
temperature :: Number -> String
temperature t
    | t < (-20.0) = "arctic"
    | t < 0.0     = "freezing"
    | t < 20.0    = "cool"
    | otherwise   = "warm"

-- Negative in case
absVal :: Integer -> Integer
absVal n = case n of
    0 -> 0
    _ -> if n < 0 then 0 - n else n

-- Negative ranges
negRange :: [Integer]
negRange = [-5 .. -1]

myMinimum :: Ord a => [a] -> a
myMinimum (x:xs) = foldl (\a b -> if a <= b then a else b) x xs
myMinimum []     = error "empty"

myMaximum :: Ord a => [a] -> a
myMaximum (x:xs) = foldl (\a b -> if a >= b then a else b) x xs
myMaximum []     = error "empty"

-- show negative numbers
main :: IO ()
main = do
    assert (sign 0 == "zero") "sign 0"
    assert (sign 1 == "one") "sign 1"
    assert (sign (-1) == "neg-one") "sign -1"
    assert (sign 42 == "positive") "sign positive"
    assert (sign (-42) == "negative") "sign negative"

    -- Arithmetic
    assert (negArith 5 3 2 == -5 + 3 + 2) "negArith: 5*(-1) + 3 - (-2) = 0"
    assert (negArith 5 3 2 == 0) "negArith == 0"
    assert ((-3) * (-4) == 12) "neg * neg"
    assert ((-7) + 10 == 3) "neg + pos"
    assert (0 - 5 == (-5)) "0 - 5"

    -- absVal
    assert (absVal 5 == 5) "abs 5"
    assert (absVal (-5) == 5) "abs -5"
    assert (absVal 0 == 0) "abs 0"

    -- temperature
    assert (temperature (-30.0) == "arctic") "arctic"
    assert (temperature (-5.0) == "freezing") "freezing"
    assert (temperature 15.0 == "cool") "cool"
    assert (temperature 25.0 == "warm") "warm"

    -- negative range
    assert (negRange == [-5, -4, -3, -2, -1]) "neg range"
    assert (length negRange == 5) "neg range length"
    assert (myMinimum negRange == (0 - 5)) "neg range min"
    assert (myMaximum negRange == (0 - 1)) "neg range max"

    -- show negative integer
    assert (show (-42 :: Integer) == "-42") "show neg int"
    assert (show (-1 :: Integer) == "-1") "show neg one"

    putStrLn "ok"
