-- GHC cgrun044: Functor laws
-- Tests that fmap obeys the functor laws for Maybe and lists

-- fmap id == id
-- fmap (f . g) == fmap f . fmap g

main :: IO ()
main = do
    -- Identity law on Maybe
    assert (fmap id (Just 42) == Just 42) "maybe id just"
    assert (fmap id (Nothing :: Maybe Integer) == Nothing) "maybe id nothing"

    -- Composition law on Maybe
    let f = (* 2)
    let g = (+ 3)
    assert (fmap (f . g) (Just 5) == (fmap f . fmap g) (Just 5)) "maybe compose just"
    assert (fmap (f . g) (Nothing :: Maybe Integer) == (fmap f . fmap g) (Nothing :: Maybe Integer)) "maybe compose nothing"

    -- Identity law on lists
    assert (fmap id [1, 2, 3] == [1, 2, 3]) "list id"
    assert (fmap id ([] :: [Integer]) == []) "list id empty"

    -- Composition law on lists
    assert (fmap (f . g) [1, 2, 3] == (fmap f . fmap g) [1, 2, 3]) "list compose"

    -- <$> is fmap
    assert ((+ 1) <$> Just 5 == Just 6) "<$> maybe"
    assert ((+ 1) <$> [1, 2, 3] == [2, 3, 4]) "<$> list"

    putStrLn "ok"
