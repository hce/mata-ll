sum' :: [Int] -> Int
sum' [] = 0
sum' (x:xs) = x + sum' xs

main :: IO ()
main = do
    assert (sum' [1, 2, 3, 4, 5] == 15) "sum [1..5] should be 15"
    assert (sum' [] == 0) "sum [] should be 0"
    assert (head [1, 2, 3] == 1) "head [1,2,3] should be 1"
    assert (length [1, 2, 3] == 3) "length [1,2,3] should be 3"
    assert (length [] == 0) "length [] should be 0"
    assert (foldl (+) 0 [1, 2, 3, 4, 5] == 15) "foldl (+) 0 [1..5] should be 15"
    assert (foldr (+) 0 [1, 2, 3] == 6) "foldr (+) 0 [1,2,3] should be 6"
    assert (head (map (+1) [1, 2, 3]) == 2) "head (map (+1) [1,2,3]) should be 2"
    assert (head (filter (>2) [1, 2, 3, 4]) == 3) "head (filter (>2) ...) should be 3"
    assert (head (take 2 [10, 20, 30]) == 10) "head (take 2 ...) should be 10"
    assert (length (take 2 [10, 20, 30]) == 2) "length (take 2 ...) should be 2"
    assert (head (reverse [1, 2, 3]) == 3) "head (reverse [1,2,3]) should be 3"
    assert (head (zipWith (+) [1, 2] [10, 20]) == 11) "zipWith (+) should work"

    -- (!!) with a negative index raises (GHC "Prelude.!!: negative
    -- index"); it used to fall through the walk loop and return the
    -- FIRST element (round-3 Q53)
    rNeg <- try (pure ([10, 20] !! (-1)) >>= \v -> print v)
    assert (case rNeg of
        Left _ -> True
        Right _ -> False) "negative list index raises"
    assert ([10, 20, 30] !! 1 == 20) "positive index unchanged"
