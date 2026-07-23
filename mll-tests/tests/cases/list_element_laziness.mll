-- Regression: a cons head is a lazy position, so a bottom stored as a list
-- element is not forced until it is actually demanded. Before the fix, cons
-- heads were built eagerly, so `[error "boom"]` ran `error` when the list was
-- constructed and `map ignore [error]` crashed. The fix suspends the head at
-- construction and forces it only at value-consumers (see the head-consumption
-- contract on __mll_head).
--
-- The dual property — laziness must be *preserved*, not traded for eagerness —
-- is checked too: infinite lists, lazy tails, self-referential lists, and
-- short-circuiting all still work, and demanded elements are still forced.

import Data.List (find, drop)

ignore :: Int -> Int
ignore _ = 5

errInt :: Int
errInt = error "unreached"

-- A cons head matched by a nested (tuple) pattern must be forced by the match.
lookupKey :: Int -> [(Int, Int)] -> Int
lookupKey _ [] = 0
lookupKey k ((k2, v) : rest) = if k == k2 then v else lookupKey k rest

main :: IO ()
main = do
    -- Bottom in a list element is NOT forced when the list is built or its
    -- spine walked, nor when the element is discarded.
    assert (length [errInt, errInt, errInt] == 3) "length ignores bottom elements"
    assert (map ignore [errInt, errInt] == [5, 5]) "map over bottom elements"
    assert (head [1, errInt] == 1) "head takes first, ignores rest"
    assert (length (errInt : [10, 20]) == 3) "cons-operator bottom head"
    assert (null [errInt] == False) "null inspects only the spine"
    assert (elem 1 [1, errInt] == True) "elem short-circuits before bottom"
    assert (ignore ([1, 2, 3] !! 1) == 5) "discarded (!!) element is lazy"
    assert (zipWith (\_ b -> b) [errInt, errInt] [10, 20] == [10, 20]) "zipWith ignores first list"

    -- Laziness over infinite / self-referential structures is preserved.
    assert (take 5 [1 ..] == [1, 2, 3, 4, 5]) "take from an infinite list"
    assert (take 3 (map (\x -> x * x) [1 ..]) == [1, 4, 9]) "map over an infinite list"
    assert (head (1 : undefined) == 1) "lazy tail is not forced"
    let fib = 1 : 1 : zipWith (+) fib (drop 1 fib)
    assert (take 7 fib == [1, 1, 2, 3, 5, 8, 13]) "self-referential lazy list"

    -- Demanded elements ARE still forced (no over-laziness).
    assert (sum [1, 2, 3, 4, 5] == 15) "sum forces the elements it adds"
    assert ([10, 20, 30] !! 1 == 20) "used (!!) element is forced"

    -- Nested pattern on a cons head (the direct fix target) reads real values.
    assert (lookupKey 2 [(1, 10), (2, 20), (3, 30)] == 20) "nested pattern on cons head"
    assert (find (\n -> n > 2) [1, 2, 3] == Just 3) "find over a list of records"

    pure ()
