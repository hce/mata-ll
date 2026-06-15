-- GHC-style test: Applicative operations

main :: IO ()
main = do
    -- pure for Maybe
    assert (pure 42 == Just 42) "pure Maybe"

    -- pure for list
    assert (pure 42 == [42]) "pure list"

    -- <*> on Maybe: apply function in Just to value in Just
    assert (Just (+1) <*> Just 5 == Just 6) "ap Maybe both Just"

    -- <*> on Maybe: Nothing propagates
    assert (Just (+1) <*> Nothing == Nothing) "ap Maybe Nothing arg"

    -- <*> on list: cartesian product application
    let fs = [(+1), (+10)]
    let xs = [100, 200]
    assert (fs <*> xs == [101, 201, 110, 210]) "ap list cartesian"

    pure ()
