-- Regression: a tuple field is a lazy position, so a bottom stored as a tuple
-- element is not forced until it is actually demanded. Before the fix, tuple
-- fields were built eagerly, so `(1, error "boom")` ran `error` when the tuple
-- was constructed and `fst (1, error "boom")` raised instead of returning 1.
-- The fix routes every tuple field through the same eager-vs-lazy weighing
-- (gen_arg) the cons head and function arguments use: cheap/total fields stay
-- eager, possibly-bottom fields are suspended and forced only at a value-
-- consumer (fst/snd/pattern destructuring, show, equality, the FFI boundary).
--
-- This file also covers the self-referential-list cons-head site (gen_expr_lazy)
-- fixed alongside the tuple work — the last of the four `:` emission sites — so
-- a bottom head in a recursive top-level list is not forced at construction.
--
-- The dual property — laziness must be *preserved*, not traded for eagerness —
-- is checked too: a demanded field still evaluates, and a pattern that inspects
-- a field forces it.

import Data.List (find)

-- An inlined tuple body: `snd (mkPair (error ...))` must not run the error,
-- so the inlined-substitution path must keep the field lazy too.
mkPair :: Integer -> (Integer, Integer)
mkPair x = (x, 1)

errInt :: Integer
errInt = error "unreached"

-- A self-referential top-level list with a bottom head (the gen_expr_lazy site).
badHeads :: [Integer]
badHeads = errInt : badHeads

-- Pattern-destructure a tuple whose first field is demanded, second discarded.
takeFirst :: (Integer, Integer) -> Integer
takeFirst (a, _) = a

-- Sum the first components of a list of pairs; the seconds are never demanded.
sumFirsts :: [(Integer, Integer)] -> Integer
sumFirsts [] = 0
sumFirsts ((a, _) : rest) = a + sumFirsts rest

main :: IO ()
main = do
    -- Bottom in a tuple field is NOT forced when the tuple is built, nor when
    -- the other field is projected.
    assert (fst (1, errInt) == 1) "fst ignores a bottom second field"
    assert (snd (errInt, 2) == 2) "snd ignores a bottom first field"
    assert (takeFirst (7, errInt) == 7) "pattern-destructure discards bottom field"

    -- Nested tuples: the bottom is buried, only a total path is demanded.
    assert (fst (fst ((3, errInt), errInt)) == 3) "nested tuple, bottom fields ignored"
    assert (snd (snd (errInt, (errInt, 9))) == 9) "nested tuple, inner second field"

    -- Tuples stored in a list: building the list and walking its spine must not
    -- force the fields; only the demanded firsts are read.
    assert (length [(1, errInt), (errInt, 2), (3, errInt)] == 3) "length over pairs with bottom fields"
    assert (sumFirsts [(1, errInt), (2, errInt), (3, errInt)] == 6) "sum firsts, seconds are bottom"
    assert (map fst [(10, errInt), (20, errInt)] == [10, 20]) "map fst over bottom seconds"

    -- The inlined-tuple-body case: `mkPair` is small enough to inline, so its
    -- `(x, 1)` body is emitted through the substitution path.
    assert (snd (mkPair errInt) == 1) "inlined tuple body keeps field lazy"

    -- The self-referential-list cons-head site (gen_expr_lazy): a bottom head in
    -- a recursive top-level list is not forced when the cell is constructed.
    assert (length (take 4 badHeads) == 4) "self-referential list bottom head not forced"

    -- Demanded fields ARE still evaluated (no over-laziness).
    assert (fst (1 + 1, errInt) == 2) "a demanded first field still evaluates"
    assert (takeFirst (2 * 3, errInt) == 6) "a demanded field through a pattern still evaluates"
    assert (snd (errInt, 4 + 5) == 9) "a demanded second field still evaluates"

    -- A pattern that inspects a field forces it (equality/show over real values).
    assert ((1, 2) == (1, 2)) "tuple equality forces both fields"
    assert (show (1, 2) == "(1, 2)") "tuple show forces both fields"
    assert (find (\p -> fst p == 2) [(1, 10), (2, 20)] == Just (2, 20)) "find over a list of pairs"

    putStrLn "All tuple field laziness tests passed!"
