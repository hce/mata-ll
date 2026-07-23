-- GHC cgrun045: Applicative patterns
-- Tests <*> on Maybe and lists

main :: IO ()
main = do
    -- Maybe Applicative
    assert ((Just (+ 1) <*> Just 5) == Just 6) "maybe <*> just"
    assert (((Nothing :: Maybe (Int -> Int)) <*> Just 5) == Nothing) "maybe <*> nothing f"
    assert ((Just (+ 1) <*> (Nothing :: Maybe Int)) == Nothing) "maybe <*> nothing x"
    assert (pure 42 == (Just 42 :: Maybe Int)) "maybe pure"

    -- List Applicative (cartesian product)
    assert (([(+ 1), (* 2)] <*> [10, 20, 30]) == [11, 21, 31, 20, 40, 60]) "list <*>"
    assert ((([] :: [Int -> Int]) <*> [1, 2, 3]) == ([] :: [Int])) "list <*> empty f"
    assert (([(+ 1)] <*> ([] :: [Int])) == ([] :: [Int])) "list <*> empty x"

    putStrLn "ok"
