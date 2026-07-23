-- GHC ds002: List patterns and cons matching
-- Tests pattern matching on list constructors

myZip :: [a] -> [b] -> [(a, b)]
myZip [] _ = []
myZip _ [] = []
myZip (x:xs) (y:ys) = (x, y) : myZip xs ys

myTakeWhile :: (a -> Bool) -> [a] -> [a]
myTakeWhile _ [] = []
myTakeWhile p (x:xs)
    | p x       = x : myTakeWhile p xs
    | otherwise  = []

myDropWhile :: (a -> Bool) -> [a] -> [a]
myDropWhile _ [] = []
myDropWhile p (x:xs)
    | p x       = myDropWhile p xs
    | otherwise  = x : xs

myLast :: [Int] -> Int
myLast [] = error "empty"
myLast (x:xs) = if length xs == 0 then x else myLast xs

myInit :: [Int] -> [Int]
myInit [] = error "empty"
myInit (x:xs) = if length xs == 0 then [] else x : myInit xs

main :: IO ()
main = do
    assert (myZip [1, 2, 3] [10, 20, 30] == [(1, 10), (2, 20), (3, 30)]) "zip"
    assert (myZip [1, 2] [10, 20, 30] == [(1, 10), (2, 20)]) "zip short"
    assert (myZip ([] :: [Int]) [1, 2] == []) "zip empty"

    assert (myTakeWhile (< 5) [1, 2, 3, 4, 5, 6] == [1, 2, 3, 4]) "takeWhile"
    assert (myDropWhile (< 5) [1, 2, 3, 4, 5, 6] == [5, 6]) "dropWhile"

    assert (myLast [1, 2, 3] == 3) "last"
    assert (myInit [1, 2, 3] == [1, 2]) "init"

    -- Cons pattern matching
    assert (head [10, 20, 30] == 10) "head"
    assert (tail [10, 20, 30] == [20, 30]) "tail"
    assert (length [1, 2, 3, 4, 5] == 5) "length"

    putStrLn "ok"
