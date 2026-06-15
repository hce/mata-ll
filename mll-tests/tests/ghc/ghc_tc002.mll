-- GHC tc002: Polymorphic list functions
-- Tests polymorphic functions on lists of different types

myLength :: [a] -> Integer
myLength [] = 0
myLength (_:xs) = 1 + myLength xs

myHead :: [a] -> a
myHead (x:_) = x
myHead []    = error "empty list"

myTail :: [a] -> [a]
myTail (_:xs) = xs
myTail []     = error "empty list"

myMap :: (a -> b) -> [a] -> [b]
myMap _ [] = []
myMap f (x:xs) = f x : myMap f xs

myFilter :: (a -> Bool) -> [a] -> [a]
myFilter _ [] = []
myFilter p (x:xs)
    | p x       = x : myFilter p xs
    | otherwise  = myFilter p xs

main :: IO ()
main = do
    -- Polymorphic on integers
    assert (myLength [1, 2, 3, 4, 5] == 5) "len int"
    assert (myHead [10, 20, 30] == 10) "head int"
    assert (myTail [10, 20, 30] == [20, 30]) "tail int"

    -- Polymorphic on strings
    assert (myLength ["a", "b", "c"] == 3) "len string"
    assert (myHead ["hello", "world"] == "hello") "head string"

    -- Polymorphic on Maybe
    assert (myLength [Just 1, Nothing, Just 3] == 3) "len maybe"

    -- Map preserves type
    assert (myMap (* 2) [1, 2, 3] == [2, 4, 6]) "map int"
    assert (myMap (\s -> s ++ "!") ["hi", "bye"] == ["hi!", "bye!"]) "map string"

    -- Filter preserves type
    assert (myFilter (> 3) [1, 2, 3, 4, 5] == [4, 5]) "filter int"

    putStrLn "ok"
