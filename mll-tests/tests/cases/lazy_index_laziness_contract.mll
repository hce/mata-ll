-- The dual half of Finding 1: the fix for the head/tail/(!!) thunk leak must
-- not regress the laziness contract the lazy-cons-heads work established.
-- A bottom in a cons position that is never consumed must stay unevaluated;
-- infinite and self-referential lists must still work; (!!) must force ONLY
-- the selected element, not every head it walks past. A spurious force is
-- observable: `error "boom: ..."` fires and the run fails with that message.

inc :: Integer -> Integer
inc x = x + 1

boom :: Integer
boom = error "boom: unconsumed element was forced"

-- The stdlib has no `repeat` (and `repeat` is a Lua keyword anyway); a
-- user-level equivalent exercises the same shape: an infinite list whose
-- every head is the same value.
repeatI :: Integer -> [Integer]
repeatI x = x : repeatI x

main :: IO ()
main = do
    -- ============================================================
    -- Bottom in an unconsumed HEAD position is never forced
    -- ============================================================
    assert (head [1, boom] == 1) "head stops before a bottom element"
    assert (length [boom] == 1) "length never looks at the one element"
    assert (length [boom, boom, boom] == 3) "length over multiple bottoms"
    assert ([1, 2, boom] !! 1 == 2) "(!!) forces only the selected element"
    assert (head (tail [1, 2, boom]) == 2) "head.tail stops before a bottom"

    -- ============================================================
    -- Bottom in an unconsumed TAIL position is never forced
    -- ============================================================
    assert (head (1 : error "boom: tail after head was forced") == 1)
        "head does not force the tail"
    let xs = 1 : 2 : error "boom: spine past the index was forced"
    assert (xs !! 1 == 2) "(!!) does not walk past the selected index"
    assert (head (tail xs) == 2) "head.tail does not walk past index 1"

    -- ============================================================
    -- Infinite structures are not fully forced by element access
    -- ============================================================
    assert (take 3 (repeatI 7) == [7, 7, 7]) "take of an infinite constant list"
    assert (repeatI 7 !! 100 == 7) "(!!) deep into an infinite constant list"
    assert (head (iterate inc 0) == 0) "head of iterate forces only the seed"
    assert (take 5 [1 ..] == [1, 2, 3, 4, 5]) "take of enumFrom"

    -- ============================================================
    -- Self-referential lists still tie the knot
    -- ============================================================
    let ones = 1 : ones
    assert (take 3 ones == [1, 1, 1]) "self-referential ones"
    let fibs = 0 : 1 : zipWith (+) fibs (tail fibs)
    assert (take 8 fibs == [0, 1, 1, 2, 3, 5, 8, 13]) "fib-style knot via tail"
    assert (fibs !! 10 == 55) "(!!) into the self-referential list"

    -- ============================================================
    -- A discarded (!!) projection stays lazy
    -- ============================================================
    -- Per-argument demand analysis must suspend the whole projection when the
    -- callee never demands it, so the bottom element is never touched.
    assert (const 5 ([1, boom, 3] !! 1) == 5) "discarded (!!) element stays lazy"

    pure ()
