-- GHC cgrun013: Map and filter on lists
-- Tests higher-order list functions

double :: Integer -> Integer
double x = x * 2

isEven :: Integer -> Bool
isEven x = x `mod` 2 == 0

main :: IO ()
main = do
    assert (map double [1, 2, 3, 4, 5] == [2, 4, 6, 8, 10]) "map double"
    assert (filter isEven [1, 2, 3, 4, 5, 6] == [2, 4, 6]) "filter even"
    assert (map double (filter isEven [1, 2, 3, 4, 5]) == [4, 8]) "map filter"
    assert (filter isEven (map double [1, 2, 3]) == [2, 4, 6]) "filter map"
    assert (map double ([] :: [Integer]) == []) "map empty"
    assert (filter isEven ([] :: [Integer]) == []) "filter empty"
    putStrLn "ok"
