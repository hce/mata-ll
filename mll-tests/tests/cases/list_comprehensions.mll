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
    assert ([x * 2 | x <- ([] :: [Int])] == []) "empty source"

    -- All filtered out
    assert ([x | x <- [1, 2, 3], x > 10] == []) "all filtered"

    -- Single element
    assert ([x | x <- [42]] == [42]) "single element"

    -- Expression body
    assert ([x <> "!" | x <- ["a", "b", "c"]] == ["a!", "b!", "c!"]) "string comp"

    -- Multiple guards
    assert ([x | x <- [1, 2, 3, 4, 5, 6], x > 2, x < 5] == [3, 4]) "multi guard"

    -- Dependent generators
    let pairs = [(x, y) | x <- [1, 2, 3], y <- [1, 2, 3], x < y]
    assert (length pairs == 3) "dependent gen count"
    assert (fst (head pairs) == 1) "dependent gen first"

    -- Pattern-matching generators
    let rs = [Right 1, Left "err", Right 2, Left "bad", Right 3]
    assert ([x | Right x <- rs] == [1, 2, 3]) "pattern gen: Right"
    assert ([s | Left s <- rs] == ["err", "bad"]) "pattern gen: Left"

    -- Wildcard pattern generator
    assert ([1 | _ <- [10, 20, 30]] == [1, 1, 1]) "wildcard gen"

    -- Tuple pattern generator
    let kvs = [(1, "a"), (2, "b"), (3, "c")]
    assert ([v | (_, v) <- kvs] == ["a", "b", "c"]) "tuple pattern gen: values"
    assert ([k | (k, _) <- kvs] == [1, 2, 3]) "tuple pattern gen: keys"

    -- Pattern generator with guard
    assert ([x | Right x <- rs, x > 1] == [2, 3]) "pattern gen + guard"

    -- Constructor pattern with multiple generators
    let xs = [Right 10, Left "x", Right 20]
    let ys = [Right 1, Right 2, Left "y"]
    assert ([a + b | Right a <- xs, Right b <- ys] == [11, 12, 21, 22]) "multi pattern gen"

    -- Multi-line layout: head, bar, and each qualifier on its own line
    let ml =
          [ (a, b)
          | a <- [1, 2, 3]
          , b <- [1, 2, 3]
          , a < b
          ]
    assert (ml == [(1, 2), (1, 3), (2, 3)]) "multi-line comprehension"

    -- Multi-line generator + guard, closing bracket dedented past the body
    let sq =
          [ x * x
          | x <- [1 .. 5]
          , even x
          ]
    assert (sq == [4, 16]) "multi-line generator + guard"

    -- Multi-line plain list literal and multi-line range
    assert ([ 1
            , 2
            , 3 ] == [1, 2, 3]) "multi-line list literal"
    assert ([ 1
            .. 4 ] == [1, 2, 3, 4]) "multi-line range"
