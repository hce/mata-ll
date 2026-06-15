-- ghc_regr013: Mixed Integer and Number arithmetic

-- Integer division and modulo
divMod' :: Integer -> Integer -> (Integer, Integer)
divMod' a b = (a `div` b, a `mod` b)

-- Number (float) operations
circleArea :: Number -> Number
circleArea r = 3.14159265 * r * r

-- Integer power via recursion
ipow :: Integer -> Integer -> Integer
ipow _ 0 = 1
ipow base n = base * ipow base (n - 1)

-- Euclidean distance squared (all integer)
distSq :: Integer -> Integer -> Integer -> Integer -> Integer
distSq x1 y1 x2 y2 = (x2 - x1) * (x2 - x1) + (y2 - y1) * (y2 - y1)

-- Number arithmetic
approxEq :: Number -> Number -> Bool
approxEq a b = (a - b) * (a - b) < 0.000001

-- Sum a list of Numbers
numSum :: [Number] -> Number
numSum [] = 0.0
numSum (x:xs) = x + numSum xs

main :: IO ()
main = do
    -- Basic Integer ops
    assert (10 `div` 3 == 3) "div"
    assert (10 `mod` 3 == 1) "mod"
    assert (divMod' 17 5 == (3, 2)) "divMod'"
    assert (fst (divMod' (-7) 2) == (-4)) "divMod' neg quot"
    assert (snd (divMod' (-7) 2) == 1) "divMod' neg rem"
    assert (ipow 2 10 == 1024) "ipow 2 10"
    assert (ipow 3 5 == 243) "ipow 3 5"
    assert (ipow 10 0 == 1) "ipow base 0"

    -- Integer comparison
    assert (distSq 0 0 3 4 == 25) "distSq 3-4-5"
    assert (distSq 1 1 4 5 == 25) "distSq shifted"

    -- Number arithmetic
    assert (circleArea 1.0 == 3.14159265) "area r=1"
    assert (approxEq (circleArea 2.0) 12.5663706) "area r=2"
    assert (approxEq (sqrt 25.0) 5.0) "sqrt 25"
    assert (approxEq (sqrt 2.0) 1.41421356) "sqrt 2"

    -- Number sums
    assert (numSum [1.0, 2.0, 3.0, 4.0, 5.0] == 15.0) "numSum"
    assert (numSum [] == 0.0) "numSum empty"
    assert (approxEq (numSum [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]) 55.0) "numSum 1..10"

    -- Integer sums
    let isum = foldl (+) 0 [1..10 :: Integer]
    assert (isum == 55) "isum"

    -- Number product
    let fprod = foldl (*) 1.0 [1.0, 2.0, 3.0, 4.0, 5.0 :: Number]
    assert (approxEq fprod 120.0) "fprod 5!"

    putStrLn "ok"
