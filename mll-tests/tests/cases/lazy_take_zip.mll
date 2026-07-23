-- GHC laziness: `take n _ | n <= 0 = []` and `zip [] _ = []` must not force
-- the list argument. A matching earlier clause never forces an argument only
-- a later clause inspects.

main :: IO ()
main = do
    -- take 0 must not force its list argument
    assert (take 0 (error "no take") == ([] :: [Int])) "take 0 (error _) = []"
    assert (take (0 - 3) (error "neg") == ([] :: [Int])) "take n _ | n<=0 = []"
    -- zip [] _ must not force the second argument
    assert (zip ([] :: [Int]) (error "no zip") == ([] :: [(Int, Int)]))
        "zip [] (error _) = []"
    -- and zip _ [] must not force the first argument's tail beyond WHNF
    assert (zip [1, 2] ([] :: [Int]) == ([] :: [(Int, Int)])) "zip _ [] = []"
    -- normal cases still work
    assert (take 2 [1, 2, 3, 4] == [1, 2]) "take still works"
    assert (zip [1, 2] [3, 4] == [(1, 3), (2, 4)]) "zip still works"
    putStrLn "lazy take/zip ok"
-- expect: lazy take/zip ok
