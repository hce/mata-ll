-- GHC cgrun029: Enum ranges
-- Tests [a..b], [a,b..c] syntax

main :: IO ()
main = do
    -- Simple range
    assert ([1..5] == [1, 2, 3, 4, 5]) "1..5"

    -- Empty range
    assert ([5..1] == ([] :: [Int])) "5..1 empty"

    -- Step range
    assert ([1, 3..10] == [1, 3, 5, 7, 9]) "1,3..10"
    assert ([10, 8..1] == [10, 8, 6, 4, 2]) "10,8..1"
    assert ([0, 5..20] == [0, 5, 10, 15, 20]) "0,5..20"

    -- Single element
    assert ([3..3] == [3]) "3..3"

    -- Negative range
    assert ([-3..3] == [-3, -2, -1, 0, 1, 2, 3]) "-3..3"

    -- Range with map
    assert (map (* 2) [1..5] == [2, 4, 6, 8, 10]) "map range"

    -- Length of range
    assert (length [1..100] == 100) "length 1..100"

    putStrLn "ok"
