-- GHC cgrun059: Set operations via sorted lists (union, intersection, difference)

-- Sets are represented as sorted, deduplicated lists

insertSorted :: (Ord a) => a -> [a] -> [a]
insertSorted x [] = [x]
insertSorted x (y:ys)
    | x < y    = x : y : ys
    | x == y   = y : ys
    | otherwise = y : insertSorted x ys

fromList :: (Ord a) => [a] -> [a]
fromList xs = foldr insertSorted [] xs

union_ :: (Ord a) => [a] -> [a] -> [a]
union_ [] ys = ys
union_ xs [] = xs
union_ (x:xs) (y:ys)
    | x < y    = x : union_ xs (y:ys)
    | x == y   = x : union_ xs ys
    | otherwise = y : union_ (x:xs) ys

intersection :: (Ord a) => [a] -> [a] -> [a]
intersection [] _ = []
intersection _ [] = []
intersection (x:xs) (y:ys)
    | x < y    = intersection xs (y:ys)
    | x == y   = x : intersection xs ys
    | otherwise = intersection (x:xs) ys

difference :: (Ord a) => [a] -> [a] -> [a]
difference [] _ = []
difference xs [] = xs
difference (x:xs) (y:ys)
    | x < y    = x : difference xs (y:ys)
    | x == y   = difference xs ys
    | otherwise = difference (x:xs) ys

member :: (Ord a) => a -> [a] -> Bool
member _ [] = False
member x (y:ys)
    | x < y    = False
    | x == y   = True
    | otherwise = member x ys

main :: IO ()
main = do
    let a = fromList ([3,1,4,1,5,9,2,6] :: [Int])
    assert (a == [1,2,3,4,5,6,9]) "fromList dedup+sort"

    let b = fromList ([2,4,6,8,10] :: [Int])
    assert (union_ a b == [1,2,3,4,5,6,8,9,10]) "union"
    assert (intersection a b == [2,4,6]) "intersection"
    assert (difference a b == [1,3,5,9]) "difference a-b"
    assert (difference b a == [8,10]) "difference b-a"

    assert (member 5 a) "5 in a"
    assert (not (member 7 a)) "7 not in a"

    let empty = ([] :: [Int])
    assert (union_ empty a == a) "union empty a"
    assert (intersection empty a == empty) "intersection empty"
    assert (difference a empty == a) "diff a empty"
    putStrLn "ok"
