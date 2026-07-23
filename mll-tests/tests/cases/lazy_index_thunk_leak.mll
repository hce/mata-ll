-- Regression for Finding 1 (release blocker; regression since v0.1.2, from
-- the lazy-cons-heads work): head / tail / (!!) on lazily-generated lists
-- must hand back forced VALUES, never raw thunks. Before the fix, a suspended
-- cons head escaped through these consumers as a bare Lua closure/table:
--   print (head (tail (iterate inc 0)))  printed "(function: 0x.., False)"
--   [1..] !! 5                           printed garbage
--   let v = iterate inc 0 !! 2 in v * 10 crashed "arithmetic on a table value"
-- The bug shows up differently on the show path (thunk representation leaks
-- into the output) and the arithmetic path (runtime crash), so every value
-- pulled out here is consumed BOTH ways.

inc :: Int -> Int
inc x = x + 1

main :: IO ()
main = do
    -- ============================================================
    -- The exact shapes from the bug report
    -- ============================================================
    assert (head (tail (iterate inc 0)) == 1) "head . tail of iterate"
    assert (iterate inc 0 !! 2 == 2) "(!!) into iterate"
    let v = iterate inc 0 !! 2
    assert (v * 10 == 20) "let-bound (!!) element used in arithmetic"
    assert ([1 ..] !! 5 == 6) "(!!) into infinite enumFrom"

    -- Same values consumed via show (the other manifestation of the leak).
    assert (show v == "2") "let-bound (!!) element used in show"
    assert (show (head (tail (iterate inc 0))) == "1") "show of head.tail of iterate"
    assert (show ([1 ..] !! 5) == "6") "show of (!!) into enumFrom"
    assert (show (iterate inc 0 !! 1, True) == "(1,True)")
        "projected element inside a shown tuple"

    -- ============================================================
    -- Literal (already-materialized) lists must keep working
    -- ============================================================
    assert (head [10, 20, 30] == 10) "head of literal list"
    assert ([100, 200, 300] !! 2 == 300) "(!!) last index of literal list"
    assert ([100, 200, 300] !! 0 == 100) "(!!) index zero of literal list"

    -- ============================================================
    -- head over lazily-mapped infinite lists
    -- ============================================================
    assert (head (map (* 2) [1 ..]) == 2) "head of map over infinite list"
    assert (head (tail (map (* 2) [1 ..])) == 4) "head.tail of map over infinite list"

    -- ============================================================
    -- take must materialize forced elements, not thunks
    -- ============================================================
    assert (take 3 (iterate inc 0) == [0, 1, 2]) "take of iterate"
    assert (show (take 3 (iterate inc 0)) == "[0,1,2]") "show of take of iterate"

    -- ============================================================
    -- Deeper head/tail chains
    -- ============================================================
    assert (head (tail (tail (iterate inc 0))) == 2) "head . tail . tail of iterate"
    assert (head (tail (tail (tail [0 ..]))) == 3) "triple tail into enumFrom"

    -- ============================================================
    -- (!!) into zipWith / filter results (lazily generated spines)
    -- ============================================================
    assert (zipWith (+) [1 ..] [10, 20, 30, 40] !! 2 == 33) "(!!) into zipWith"
    assert (filter (\x -> x `mod` 2 == 0) [1 ..] !! 3 == 8)
        "(!!) into filter of an infinite list"
    let w = zipWith (*) (iterate inc 1) [10, 10, 10] !! 1
    assert (w + 1 == 21) "zipWith element used in arithmetic"
    assert (show w == "20") "zipWith element used in show"

    pure ()
