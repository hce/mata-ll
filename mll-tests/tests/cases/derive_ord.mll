-- Tests for derived Ord on types with fields.
-- Haskell semantics: compare by constructor declaration order first;
-- for equal constructors, compare fields left-to-right lexicographically.
-- Regression: derived Ord used to compare constructor indices ONLY,
-- so Q 1 < Q 2 was False and compare (Q 1) (Q 2) was EQ.

-- Single-field, single-constructor (the original bug reproducer)
data Q = Q Integer
    deriving (Show, Eq, Ord)

-- Multi-field plus nullary constructors on both sides
data T = A | B Integer String | C
    deriving (Show, Eq, Ord)

-- Recursive ADT field (the type itself appears as a field)
data Tree = Leaf | Node Tree Integer Tree
    deriving (Show, Eq, Ord)

-- Parameterized ADT used at concrete types
data Pair a b = MkPair a b
    deriving (Show, Eq, Ord)

-- Plain enum: must keep pure index ordering
data Sev = Low | Mid | High
    deriving (Show, Eq, Ord)

-- Fielded ADT whose fields are themselves derived-Ord ADTs
data Item = Item Sev Q
    deriving (Show, Eq, Ord)

isLT :: Ordering -> Bool
isLT o = o == LT

insert :: Item -> [Item] -> [Item]
insert x [] = [x]
insert x (y:ys) = if x <= y then x : y : ys else y : insert x ys

isort :: [Item] -> [Item]
isort = foldr insert []

main :: IO ()
main = do
    -- Single field: fields decide, not just the constructor
    assert (compare (Q 1) (Q 2) == LT) "q compare lt"
    assert (compare (Q 2) (Q 1) == GT) "q compare gt"
    assert (compare (Q 3) (Q 3) == EQ) "q compare eq"
    assert (Q 1 < Q 2) "q lt"
    assert (not (Q 2 < Q 1)) "q not lt"
    assert (Q 2 > Q 1) "q gt"
    assert (Q 1 <= Q 1) "q le refl"
    assert (Q 1 <= Q 2) "q le"
    assert (Q 2 >= Q 2) "q ge refl"
    assert (not (Q 1 >= Q 2)) "q not ge"

    -- Multi-field: lexicographic, first field wins over second
    assert (compare (B 1 "x") (B 1 "y") == LT) "b second field lt"
    assert (compare (B 2 "a") (B 1 "z") == GT) "b first field wins"
    assert (compare (B 1 "x") (B 1 "x") == EQ) "b eq"
    assert (B 1 "x" < B 1 "y") "b lt op"
    assert (B 1 "z" <= B 2 "a") "b le op"

    -- Mixed nullary/fielded: constructor order first, nullary EQ to itself
    assert (compare A (B 0 "") == LT) "a lt b"
    assert (compare C (B 9 "") == GT) "c gt b"
    assert (compare A A == EQ) "a eq a"
    assert (compare C C == EQ) "c eq c"
    assert (A < C) "a lt c"
    assert (A <= A) "a le a"

    -- Recursive fields: nested trees compared structurally
    assert (compare (Node Leaf 1 Leaf) (Node Leaf 2 Leaf) == LT) "tree mid lt"
    assert (compare (Node (Node Leaf 5 Leaf) 1 Leaf) (Node Leaf 1 Leaf) == GT) "tree left field wins"
    assert (Node Leaf 3 Leaf < Node Leaf 3 (Node Leaf 0 Leaf)) "tree right tiebreak"
    assert (compare Leaf Leaf == EQ) "leaf eq"
    assert (Leaf < Node Leaf 0 Leaf) "leaf lt node"

    -- Parameterized ADT at concrete types (per-instantiation dispatch)
    assert (compare (MkPair (1 :: Integer) "b") (MkPair 1 "a") == GT) "pair snd gt"
    assert (compare (MkPair (1 :: Integer) "a") (MkPair 2 "a") == LT) "pair fst lt"
    assert (MkPair (1 :: Integer) (2 :: Integer) < MkPair 1 3) "pair int lt"
    assert (MkPair "x" (1 :: Integer) >= MkPair "x" 1) "pair ge eq"
    -- nested parameterized instantiation
    assert (isLT (compare (MkPair (1 :: Integer) (MkPair (2 :: Integer) (3 :: Integer)))
                          (MkPair 1 (MkPair 2 4)))) "pair nested lt"

    -- Enum ordering unchanged: pure declaration-index comparison
    assert (compare Low Mid == LT) "enum lt"
    assert (compare High Mid == GT) "enum gt"
    assert (compare Mid Mid == EQ) "enum eq"
    assert (Low < Mid && Mid < High) "enum chain"
    assert (High >= Low) "enum ge"

    -- Fields that are themselves derived-Ord ADTs, exercised via a sort
    assert (compare (Item Low (Q 9)) (Item High (Q 0)) == LT) "item enum field first"
    assert (compare (Item Mid (Q 2)) (Item Mid (Q 1)) == GT) "item adt field tiebreak"
    let sorted = isort [Item High (Q 1), Item Low (Q 2), Item Mid (Q 3), Item Low (Q 1), Item High (Q 0)]
    assert (sorted == [Item Low (Q 1), Item Low (Q 2), Item Mid (Q 3), Item High (Q 0), Item High (Q 1)]) "isort items"

    putStrLn "all derived Ord tests passed"
