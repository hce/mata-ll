-- GHC ds012: Enum range desugaring
-- [1..n], [1,3..10], [n..], take from infinite ranges

appendList :: [a] -> [a] -> [a]
appendList [] ys     = ys
appendList (x:xs) ys = x : appendList xs ys

main :: IO ()
main = do
    -- Basic range
    assert ([1..5] == [1, 2, 3, 4, 5]) "range 1..5"
    assert ([0..0] == [0]) "range singleton"
    assert ([3..1] == ([] :: [Int])) "range empty descending"

    -- Step range
    assert ([1,3..9]  == [1, 3, 5, 7, 9]) "step odd"
    assert ([0,2..10] == [0, 2, 4, 6, 8, 10]) "step even"
    assert ([1,4..10] == [1, 4, 7, 10]) "step 3"
    assert ([10,8..2] == [10, 8, 6, 4, 2]) "step down"

    -- Infinite range via take
    let inf = [1..]
    assert (take 5 inf == [1, 2, 3, 4, 5]) "take from infinite"
    assert (take 0 inf == ([] :: [Int])) "take 0 from infinite"

    -- Infinite step range via take
    let odds = [1,3..]
    assert (take 5 odds == [1, 3, 5, 7, 9]) "take odds"
    let evens = [0,2..]
    assert (take 4 evens == [0, 2, 4, 6]) "take evens"

    -- sum of range
    assert (foldl (+) 0 [1..10] == 55) "sum 1..10"
    assert (foldl (+) 0 [1..100] == 5050) "sum 1..100"

    -- length of range
    assert (length [1..20] == 20) "length 1..20"
    assert (length [1,3..19] == 10) "length odd 1..19"

    -- elem in range
    assert ((7 `elem` [1..10]) == True)  "elem in range"
    assert ((11 `elem` [1..10]) == False) "elem not in range"

    putStrLn "ok"
