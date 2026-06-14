-- Comprehensive list comprehension tests

main :: IO ()
main = do
    -- Simple map
    assert ([x * 2 | x <- [1, 2, 3]] == [2, 4, 6]) "map"

    -- Filter
    assert ([x | x <- [1, 2, 3, 4, 5], x > 3] == [4, 5]) "filter"

    -- Map + filter
    assert ([x * x | x <- [1, 2, 3, 4], x > 2] == [9, 16]) "map+filter"

    -- Cartesian product
    assert ([x + y | x <- [1, 2], y <- [10, 20]] == [11, 21, 12, 22]) "cartesian"

    -- Nested with guard
    assert ([x * y | x <- [1, 2, 3], y <- [1, 2, 3], x /= y] == [2, 3, 2, 6, 3, 6]) "nested guard"

    -- Empty source
    assert ([x * 2 | x <- ([] :: [Integer])] == []) "empty source"

    -- All filtered out
    assert ([x | x <- [1, 2, 3], x > 10] == []) "all filtered"

    -- Single element
    assert ([x | x <- [42]] == [42]) "single element"

    -- Expression body
    assert ([x ++ "!" | x <- ["a", "b", "c"]] == ["a!", "b!", "c!"]) "string comp"

    -- Multiple guards
    assert ([x | x <- [1, 2, 3, 4, 5, 6], x > 2, x < 5] == [3, 4]) "multi guard"

    -- Dependent generators
    let pairs = [(x, y) | x <- [1, 2, 3], y <- [1, 2, 3], x < y]
    assert (length pairs == 3) "dependent gen count"
    assert (fst (head pairs) == 1) "dependent gen first"
