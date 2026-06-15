-- Tests for equality on nested container types

main :: IO ()
main = do
    -- List of tuples
    assert ([(1, 2)] == [(1, 2)]) "list of tuple eq"
    assert ([(1, 2), (3, 4)] == [(1, 2), (3, 4)]) "list of tuple eq multi"
    assert ([(1, 2)] /= [(1, 3)]) "list of tuple neq"
    assert ([(1, 2)] /= [(1, 2), (3, 4)]) "list of tuple diff len"

    -- List of Maybe
    assert ([Just 1, Nothing] == [Just 1, Nothing]) "list of maybe eq"
    assert ([Just 1] /= [Just 2]) "list of maybe neq"
    assert ([Nothing] == [Nothing :: Maybe Integer]) "list of nothing eq"

    -- Tuple of lists
    assert (([1, 2], [3, 4]) == ([1, 2], [3, 4])) "tuple of lists eq"
    assert (([1, 2], [3, 4]) /= ([1, 2], [3, 5])) "tuple of lists neq"

    -- Tuple of Maybe
    assert ((Just 1, Just 2) == (Just 1, Just 2)) "tuple of maybe eq"
    assert ((Just 1, Nothing) /= (Just 1, Just 2)) "tuple of maybe neq"

    -- Nested lists
    assert ([[1, 2], [3]] == [[1, 2], [3]]) "list of list eq"
    assert ([[1, 2], [3]] /= [[1, 2], [4]]) "list of list neq"

    -- 3-tuple with mixed types
    assert ((1, "a", True) == (1, "a", True)) "3-tuple eq"
    assert ((1, "a", True) /= (1, "b", True)) "3-tuple neq"

    -- List of 3-tuples
    assert ([(1, "a", True)] == [(1, "a", True)]) "list of 3-tuple eq"

    -- Maybe of tuple
    assert (Just (1, 2) == Just (1, 2)) "maybe of tuple eq"
    assert (Just (1, 2) /= Just (1, 3)) "maybe of tuple neq"

    -- Deeply nested: Maybe of list
    assert (Just [1, 2, 3] == Just [1, 2, 3]) "maybe of list eq"
    assert (Just [1, 2] /= Just [1, 3]) "maybe of list neq"

    putStrLn "."
