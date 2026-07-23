-- GHC cgrun031: Operator sections and partial application
-- Tests operator sections (+ 1), (* 2)

addOne :: Int -> Int
addOne = (+ 1)

timesTwo :: Int -> Int
timesTwo = (* 2)

main :: IO ()
main = do
    -- Left section
    assert (map (+ 1) [1, 2, 3] == [2, 3, 4]) "left +"
    assert (map (* 2) [1, 2, 3] == [2, 4, 6]) "left *"

    -- Section in filter
    assert (filter (> 3) [1, 2, 3, 4, 5] == [4, 5]) "section filter >"
    assert (filter (< 3) [1, 2, 3, 4, 5] == [1, 2]) "section filter <"

    -- Partial application
    let add5 = (+ 5)
    assert (add5 10 == 15) "partial +"

    -- map with section
    assert (map (* 10) [1, 2, 3] == [10, 20, 30]) "section *10"

    -- Named section bindings
    assert (addOne 5 == 6) "addOne"
    assert (timesTwo 5 == 10) "timesTwo"

    putStrLn "ok"
