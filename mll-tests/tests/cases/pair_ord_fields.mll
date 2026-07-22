-- Regression: parameterized-instance method dispatch on FIELDS.
-- The Ord/Eq instances for `Pair a b` are chosen by the head constructor
-- (structured instance identity), and their still-polymorphic method bodies
-- must be specialized at each concrete use so the field comparisons resolve
-- to the fields' own instances — recursively, including nested Pairs and
-- structural fields (lists, Maybe). The old resolver related use types to
-- instances through Display strings ("Pair a b" vs "Pair Integer String"),
-- which no exact lookup could hit.

data Pair a b = MkPair a b
    deriving (Show, Eq, Ord)

data Color = Red | Green | Blue
    deriving (Show, Eq, Ord)

main :: IO ()
main = do
    -- Direct dispatch at two different instantiations of the same head.
    assert (MkPair (1 :: Integer) "b" == MkPair 1 "b") "eq Pair Integer String"
    assert (MkPair "a" (2 :: Integer) /= MkPair "b" 2) "neq Pair String Integer"
    assert (compare (MkPair (1 :: Integer) (2 :: Integer)) (MkPair 1 3) == LT) "ord second field"
    assert (compare (MkPair (2 :: Integer) (0 :: Integer)) (MkPair 1 9) == GT) "ord first field wins"

    -- Field methods on user ADT fields.
    assert (compare (MkPair Red (1 :: Integer)) (MkPair Blue 0) == LT) "ord Color field"
    assert (MkPair Green Green == MkPair Green Green) "eq Color fields"

    -- Nested Pair: the outer instance's field comparison must resolve to the
    -- inner Pair instance, specialized at ITS concrete field types.
    let n1 = MkPair (MkPair (1 :: Integer) "x") Red
    let n2 = MkPair (MkPair (1 :: Integer) "y") Red
    assert (n1 /= n2) "neq nested Pair"
    assert (compare n1 n2 == LT) "ord nested Pair via inner String field"
    assert (compare n2 n1 == GT) "ord nested Pair reversed"

    -- Structural fields inside the Pair.
    assert (MkPair [1 :: Integer, 2] (Just Red) == MkPair [1, 2] (Just Red)) "eq list and Maybe fields"
    assert (MkPair [1 :: Integer] Nothing /= MkPair [1] (Just Blue)) "neq Maybe field"

    -- Show goes through the same per-type specialization.
    assert (show n1 == "MkPair (MkPair 1 \"x\") Red") "show nested Pair"
    putStrLn "ok"
