-- GHC-style test: fmap over various functors

double :: Integer -> Integer
double x = x * 2

inc :: Integer -> Integer
inc x = x + 1

main :: IO ()
main = do
    -- fmap over Maybe
    let r1 = fmap double (Just 21)
    assert (r1 == Just 42) "fmap double Just"

    let r2 = fmap double Nothing
    assert (r2 == Nothing) "fmap double Nothing"

    -- fmap over list
    let r3 = fmap double [1, 2, 3]
    assert (r3 == [2, 4, 6]) "fmap double list"

    -- fmap with section
    let r4 = fmap (+10) [1, 2, 3]
    assert (r4 == [11, 12, 13]) "fmap section list"

    -- fmap id == id (functor identity law)
    let ys = [10, 20, 30]
    assert (fmap id ys == ys) "functor identity law"

    -- nested fmap
    let nested = [Just 1, Nothing, Just 3]
    let r5 = fmap (fmap (+10)) nested
    assert (r5 == [Just 11, Nothing, Just 13]) "nested fmap"

    -- fmap composition: fmap f . fmap g == fmap (f . g)
    let xs = [1, 2, 3]
    let lhs = fmap inc (fmap double xs)
    let rhs = fmap (inc . double) xs
    assert (lhs == rhs) "functor composition law"

    pure ()
