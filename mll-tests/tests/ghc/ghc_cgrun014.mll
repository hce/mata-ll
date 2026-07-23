-- GHC cgrun014: Foldr and foldl
-- Tests fold operations on lists

mySum :: [Int] -> Int
mySum xs = foldl (\acc x -> acc + x) 0 xs

myProduct :: [Int] -> Int
myProduct xs = foldl (\acc x -> acc * x) 1 xs

myReverse :: [Int] -> [Int]
myReverse xs = foldl (\acc x -> x : acc) [] xs

main :: IO ()
main = do
    assert (mySum [1, 2, 3, 4, 5] == 15) "sum"
    assert (myProduct [1, 2, 3, 4, 5] == 120) "product"
    assert (myReverse [1, 2, 3] == [3, 2, 1]) "reverse"
    assert (foldr (\x acc -> x : acc) ([] :: [Int]) [1, 2, 3] == [1, 2, 3]) "foldr cons"
    assert (mySum [] == 0) "sum empty"
    assert (myProduct [] == 1) "product empty"
    putStrLn "ok"
