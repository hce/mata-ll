-- GHC's Ord class defaults: an instance may define `compare` alone or `<=`
-- alone and the other six methods follow (compare via `==` and `<=`; the
-- comparisons, max and min via compare). The builtin Ord class carried no
-- default bodies, so such an instance failed at every use of a missing
-- method with "No instance for '<' on type 'T'" — thirteen errors for one
-- compare-only instance, including one inside the Prelude's sort.

data T = T Int deriving (Show, Eq)

-- Reverse order through `compare` only.
instance Ord T where
    compare (T a) (T b) = compare b a

data U = U Int deriving (Show, Eq)

-- Natural order through `<=` only.
instance Ord U where
    (<=) (U a) (U b) = a <= b

data V = V String deriving (Show, Eq)

-- Mixed: `compare` given, `max` overridden.
instance Ord V where
    compare (V a) (V b) = compare a b
    max (V a) (V b) = V (a <> b)

main :: IO ()
main = do
    print (T 1 < T 2, T 1 <= T 2, T 1 > T 2, T 1 >= T 2, T 2 >= T 2)
    print (compare (T 1) (T 2), compare (T 5) (T 5))
    print (max (T 1) (T 2), min (T 1) (T 2))
    print (sort [T 3, T 1, T 2])
    print (maximum [T 3, T 1, T 2], minimum [T 3, T 1, T 2])
    print (U 1 < U 2, U 2 < U 1, U 1 <= U 1, U 3 > U 2, U 2 >= U 3)
    print (compare (U 1) (U 2), compare (U 2) (U 1), compare (U 4) (U 4))
    print (max (U 1) (U 2), min (U 1) (U 2), sort [U 3, U 1, U 2])
    print (max (V "a") (V "b"), min (V "a") (V "b"), V "a" < V "b")
    print (sort [V "c", V "a", V "b"])
