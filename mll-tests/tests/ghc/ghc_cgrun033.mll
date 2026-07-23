-- GHC cgrun033: Zipwith and zip patterns
-- Tests zipWith on lists

main :: IO ()
main = do
    assert (zipWith (+) [1, 2, 3] [10, 20, 30] == [11, 22, 33]) "zipWith +"
    assert (zipWith (*) [1, 2, 3] [4, 5, 6] == [4, 10, 18]) "zipWith *"
    assert (zipWith (+) [1, 2] [10, 20, 30] == [11, 22]) "zipWith short left"
    assert (zipWith (+) [1, 2, 3] [10, 20] == [11, 22]) "zipWith short right"
    assert (zipWith (+) ([] :: [Int]) [1, 2] == []) "zipWith empty"

    -- Dot product
    let dot xs ys = foldl (+) 0 (zipWith (*) xs ys)
    assert (dot [1, 2, 3] [4, 5, 6] == 32) "dot product"

    -- Pairwise max
    let pmax xs ys = zipWith (\a b -> if a > b then a else b) xs ys
    assert (pmax [1, 5, 3] [4, 2, 6] == [4, 5, 6]) "pairwise max"

    putStrLn "ok"
