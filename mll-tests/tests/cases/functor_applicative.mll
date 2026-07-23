-- Test Functor and Applicative typeclasses

showEither :: Either String Int -> String
showEither (Left s) = "Left " <> s
showEither (Right n) = "Right " <> show n

main :: IO ()
main = do
    -- Functor: fmap on Maybe
    assert (fmap (+1) (Just 5) == Just 6) "fmap Maybe Just"
    assert (fmap (+1) Nothing == Nothing) "fmap Maybe Nothing"

    -- Functor: fmap on list
    assert (fmap (+1) [1, 2, 3] == [2, 3, 4]) "fmap list"

    -- Functor: fmap on Either (no Eq, check via show)
    assert (showEither (fmap (+1) (Right 5)) == "Right 6") "fmap Either Right"
    assert (showEither (fmap (+1) (Left "err")) == "Left err") "fmap Either Left"

    -- Functor: <$> operator (alias for fmap)
    assert (((+1) <$> Just 5) == Just 6) "<$> Maybe"
    assert (((+1) <$> [1, 2, 3]) == [2, 3, 4]) "<$> list"

    -- Functor: fmap on IO
    let io_action = fmap (+1) (pure 41) :: IO Int
    result <- io_action
    assert (result == 42) "fmap IO"

    -- Applicative: pure on Maybe and list
    assert (pure 5 == Just 5) "pure Maybe"
    assert (pure 5 == [5]) "pure list"

    -- Applicative: <*> on Maybe
    assert ((Just (+1) <*> Just 5) == Just 6) "<*> Maybe Just"
    assert ((Just (+1) <*> Nothing) == Nothing) "<*> Maybe Nothing"

    -- Applicative: <*> on list
    assert (([(+1), (*2)] <*> [10, 20]) == [11, 21, 20, 40]) "<*> list"

    -- Applicative: <*> on IO
    let io_ap = pure (+1) <*> pure 99 :: IO Int
    r <- io_ap
    assert (r == 100) "<*> IO"

    -- Monad: return resolves per-type
    let mx = return 42 :: Maybe Int
    assert (mx == Just 42) "return Maybe"

    pure ()
