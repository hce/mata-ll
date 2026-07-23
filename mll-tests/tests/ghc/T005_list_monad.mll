-- GHC-style test: list Functor/Applicative operations

main :: IO ()
main = do
    -- List return
    let r1 = return 42 :: [Int]
    assert (r1 == [42]) "list return"

    -- fmap on list
    assert (fmap (*2) [1, 2, 3] == [2, 4, 6]) "fmap list"

    -- pure for list
    assert (pure 99 == [99]) "pure list"

    -- <$> on list
    assert (((*3) <$> [1, 2, 3]) == [3, 6, 9]) "<$> list"

    -- <*> on list
    assert (([(+1), (*2)] <*> [10, 20]) == [11, 21, 20, 40]) "<*> list"

    -- <*> with empty
    assert (([(+1)] <*> []) == ([] :: [Int])) "<*> list empty"

    pure ()
