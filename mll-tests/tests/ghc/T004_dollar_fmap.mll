-- GHC-style test: <$> operator in various contexts

neg :: Integer -> Integer
neg x = 0 - x

main :: IO ()
main = do
    -- <$> on Maybe
    assert ((neg <$> Just 5) == Just (0 - 5)) "<$> Maybe"
    assert ((neg <$> Nothing) == Nothing) "<$> Nothing"

    -- <$> on list
    assert (((*3) <$> [1, 2, 3]) == [3, 6, 9]) "<$> list"

    -- <$> chained: (+1) <$> (*2) <$> xs
    -- parses as (+1) <$> ((*2) <$> xs) due to left-assoc infixl 4
    assert (((+1) <$> ((*2) <$> [1, 2, 3])) == [3, 5, 7]) "<$> chained"

    -- <$> is same as fmap
    let xs = [10, 20, 30]
    assert (fmap (+1) xs == ((+1) <$> xs)) "<$> equals fmap"

    pure ()
