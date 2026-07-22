-- GHC cgrun045: Applicative patterns
-- Tests <*> on Maybe and lists

main :: IO ()
main = do
    -- Maybe Applicative
    assert ((Just (+ 1) <*> Just 5) == Just 6) "maybe <*> just"
    assert (((Nothing :: Maybe (Integer -> Integer)) <*> Just 5) == Nothing) "maybe <*> nothing f"
    assert ((Just (+ 1) <*> (Nothing :: Maybe Integer)) == Nothing) "maybe <*> nothing x"
    assert (pure 42 == (Just 42 :: Maybe Integer)) "maybe pure"

    -- List Applicative (cartesian product)
    assert (([(+ 1), (* 2)] <*> [10, 20, 30]) == [11, 21, 31, 20, 40, 60]) "list <*>"
    assert ((([] :: [Integer -> Integer]) <*> [1, 2, 3]) == ([] :: [Integer])) "list <*> empty f"
    assert (([(+ 1)] <*> ([] :: [Integer])) == ([] :: [Integer])) "list <*> empty x"

    putStrLn "ok"
