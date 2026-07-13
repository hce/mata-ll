-- Regression: head / (!!) must return the ELEMENT, never a raw thunk.
--
-- The lazy-cons-heads work stores an unevaluated thunk in the head slot of a
-- cons cell. The runtime's `head` and `__mll_list_index` returned that slot
-- raw (`l[1]`), violating the WHNF-return invariant every compiled function
-- obeys (a function never returns an unforced thunk). A call site that
-- wrapped the result in its own thunk — `print (head (tail xs))` thunks the
-- print argument — then held a thunk-inside-a-thunk, and `__force`, which by
-- contract unwraps exactly one level, handed the INNER thunk out as if it
-- were the value: show rendered it as `(function: 0x…, False)` and
-- arithmetic crashed with "attempt to perform arithmetic on a table value".
--
-- The fix forces the head at the point of RETURN in `head` and
-- `__mll_list_index` (they are value-consumers under the head-consumption
-- contract). The dual property is checked below too: forcing on return must
-- NOT trade away laziness — only the returned element is forced, never its
-- neighbours, and a merely stored `head`/`!!` application stays a thunk
-- until demanded.

inc :: Integer -> Integer
inc x = x + 1

ones :: [Integer]
ones = 1 : ones

main :: IO ()
main = do
    -- The original bug reports: nested projections in a thunked argument.
    assert (head (tail (iterate inc 0)) == 1) "head of tail of iterate"
    assert (([1..] !! 5) == 6) "index into infinite range"

    -- Arithmetic on a let-bound (thunked) projection: crashed before the fix.
    let v = iterate inc 0 !! 2
    assert (v * 10 == 20) "arithmetic on thunked (!!) result"

    -- show must render the element, not a thunk pair.
    assert (show (head (tail (iterate inc 0))) == "1") "show of projected head"

    -- ── Laziness preservation ─────────────────────────────────────────
    -- head forces ONLY the first element; a bottom neighbour is untouched.
    assert (head [1, error "boom-head"] == 1) "head leaves later elements alone"

    -- (!!) forces ONLY the indexed element; bottoms before it are skipped.
    assert (([10, error "boom-index", 30] !! 2) == 30) "(!!) skips earlier bottoms"

    -- The spine is consumable without touching any element.
    assert (length [error "boom-length"] == 1) "length never forces heads"
    assert (length (error "boom-spine" : [5]) == 2) "bottom head does not poison the spine"

    -- Infinite / self-referential lists still work through the projections.
    assert (take 3 (iterate inc 7) == [7, 8, 9]) "take on infinite iterate"
    assert (take 4 ones == [1, 1, 1, 1]) "take on self-referential list"
    assert (head ones == 1) "head of self-referential list"
    assert ((error "boom-unconsumed" : [5]) !! 1 == 5) "(!!) past an unconsumed bottom head"

    putStrLn "lazy head projection ok"
