-- GHC-style test: Maybe operations (Functor/Applicative)

main :: IO ()
main = do
    -- return in Maybe context
    let r = return 42 :: Maybe Integer
    assert (r == Just 42) "return Maybe"

    -- fmap on Maybe
    assert (fmap (+1) (Just 5) == Just 6) "fmap Maybe Just"
    assert (fmap (+1) Nothing == Nothing) "fmap Maybe Nothing"

    -- <$> on Maybe
    assert (((*2) <$> Just 10) == Just 20) "<$> Maybe"

    -- pure on Maybe
    assert (pure 99 == Just 99) "pure Maybe"

    -- <*> on Maybe
    assert ((Just (+1) <*> Just 5) == Just 6) "<*> Maybe"
    assert ((Just (+1) <*> Nothing) == Nothing) "<*> Maybe Nothing"

    pure ()
