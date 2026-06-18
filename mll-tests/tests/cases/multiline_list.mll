-- Regression test: multi-line list literals
-- Lists can span multiple lines with leading commas.
-- (Bug: parser choked on Indent tokens between list elements)

-- Basic multi-line list
numbers :: [Integer]
numbers = [ 1, 2, 3
           , 4, 5, 6
           , 7, 8, 9
           ]

-- Single element per line
vertical :: [Integer]
vertical = [ 10
           , 20
           , 30
           ]

-- Nested expressions in multi-line list
exprs :: [Integer]
exprs = [ 1 + 2
        , 3 * 4
        , 5 - 1
        ]

-- Multi-line list of tuples
pairs :: [(Integer, Integer)]
pairs = [ (1, 2)
        , (3, 4)
        , (5, 6)
        ]

-- Multi-line list of strings
names :: [String]
names = [ "alice"
        , "bob"
        , "carol"
        ]

-- Trailing element on same line as bracket
mixed :: [Integer]
mixed = [ 100
        , 200, 300 ]

main :: IO ()
main = do
    assert (numbers == [1, 2, 3, 4, 5, 6, 7, 8, 9]) "multi-line basic"
    assert (length numbers == 9) "multi-line length"
    assert (vertical == [10, 20, 30]) "vertical list"
    assert (exprs == [3, 12, 4]) "multi-line expressions"
    assert (pairs == [(1, 2), (3, 4), (5, 6)]) "multi-line tuples"
    assert (names == ["alice", "bob", "carol"]) "multi-line strings"
    assert (mixed == [100, 200, 300]) "mixed layout"
    putStrLn "All multi-line list tests passed!"
