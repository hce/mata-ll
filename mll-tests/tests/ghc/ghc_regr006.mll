-- ghc_regr006: Large enum type with Ord (10+ constructors)

data Priority = P0 | P1 | P2 | P3 | P4 | P5 | P6 | P7 | P8 | P9 | P10
    deriving (Show, Eq, Ord, Enum, Bounded)

toInt :: Priority -> Integer
toInt P0  = 0
toInt P1  = 1
toInt P2  = 2
toInt P3  = 3
toInt P4  = 4
toInt P5  = 5
toInt P6  = 6
toInt P7  = 7
toInt P8  = 8
toInt P9  = 9
toInt P10 = 10

myMin :: Ord a => a -> a -> a
myMin a b = if a <= b then a else b

myMax :: Ord a => a -> a -> a
myMax a b = if a >= b then a else b

myLast :: [a] -> a
myLast (x:xs) = case xs of
    [] -> x
    _  -> myLast xs
myLast [] = error "empty"

myMinimum :: Ord a => [a] -> a
myMinimum (x:xs) = foldl (\a b -> if a <= b then a else b) x xs
myMinimum []     = error "empty"

myMaximum :: Ord a => [a] -> a
myMaximum (x:xs) = foldl (\a b -> if a >= b then a else b) x xs
myMaximum []     = error "empty"

main :: IO ()
main = do
    assert (P0 < P1) "P0 < P1"
    assert (P5 < P10) "P5 < P10"
    assert (P10 > P9) "P10 > P9"
    assert (P3 == P3) "P3 == P3"
    assert (P2 /= P7) "P2 /= P7"
    assert (P0 <= P0) "P0 <= P0"
    assert (P4 >= P3) "P4 >= P3"
    assert (myMin P3 P7 == P3) "min P3 P7"
    assert (myMax P3 P7 == P7) "max P3 P7"
    assert (myMin P10 P0 == P0) "min P10 P0"
    assert (minBound == P0) "minBound"
    assert (maxBound == P10) "maxBound"
    let all11 = [P0 .. P10]
    assert (length all11 == 11) "length enum range"
    assert (head all11 == P0) "first enum"
    assert (myLast all11 == P10) "last enum"
    assert (toInt P5 == 5) "toInt P5"
    assert (toInt P9 == 9) "toInt P9"
    assert (toInt P10 == 10) "toInt P10"
    let ps = [P9, P2, P5, P0, P7]
    assert (myMinimum ps == P0) "minimum"
    assert (myMaximum ps == P9) "maximum"
    putStrLn "ok"
