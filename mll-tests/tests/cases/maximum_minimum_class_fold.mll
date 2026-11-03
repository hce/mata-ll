-- `maximum`/`minimum` are GHC's `foldl1' max` / `foldl1' min`: the Ord
-- instance's own `max`/`min`, called as `max acc x` over the elements. So a
-- tie yields the LAST of equal maxima and the FIRST of equal minima (the
-- defaults `max x y = if x <= y then y else x`, `min x y = if x <= y then x
-- else y`), an instance's own `max`/`min` are used whether lawful or not,
-- and at Number a NaN is never `<=` anything and 0.0 <= -0.0 holds. Also:
-- a `<=`-only instance, Maybe and Either as the Foldable, lexicographic
-- ties on tuples and strings, a 10^5-element list (a strict fold, no stack
-- growth), the empty-structure error through `catch` (on a Maybe and an
-- Either: GHC's list instance says "Prelude.maximum: empty list" instead,
-- a message mata-ll's Foldable-generic definition cannot give per instance;
-- see HASKDIFF), and the same folds inside a dictionary-passing body (an
-- existential's hidden Ord).
-- Before, the folds were hand-written `>`/`<` tests: `maximum [P 1 "a", P 1
-- "b"]` printed `P 1 "a"`, `maximum [M 1, M 2]` ignored the instance's
-- `max`, and `minimum [1.0, nan, 2.0]` printed `1.0` where GHC prints `2.0`.

data P = P Int String deriving (Show)
instance Eq P where
    (==) (P a _) (P b _) = a == b
instance Ord P where
    compare (P a _) (P b _) = compare a b

data Q = Q Int String deriving (Show)
instance Eq Q where
    (==) (Q a _) (Q b _) = a == b
instance Ord Q where
    (<=) (Q a _) (Q b _) = a <= b

data M = M Int deriving (Show, Eq)
instance Ord M where
    compare (M a) (M b) = compare a b
    max _ _ = M 99
    min _ _ = M (-99)

data V = V String deriving (Show, Eq)
instance Ord V where
    compare (V a) (V b) = compare a b
    max (V a) (V b) = V (a <> b)

data Bag = forall a. (Show a, Ord a) => Bag [a]

extremes :: Bag -> String
extremes (Bag xs) = show (maximum xs) <> " " <> show (minimum xs)

dv :: Number -> Number -> Number
dv a b = a / b

main :: IO ()
main = do
    print (maximum [P 1 "a", P 1 "b", P 0 "c"])
    print (minimum [P 1 "a", P 0 "b", P 0 "c"])
    print (maximum [P 2 "b", P 2 "c"], minimum [P 2 "b", P 2 "c"])
    print (maximum [Q 1 "a", Q 1 "b", Q 0 "c"], minimum [Q 1 "a", Q 0 "b", Q 0 "c"])
    print (maximum [0.0, -0.0 :: Number], minimum [0.0, -0.0 :: Number], maximum [-0.0, 0.0 :: Number], minimum [-0.0, 0.0 :: Number])
    print (maximum [M 1, M 2], minimum [M 1, M 2], maximum [M 5], minimum [M 5])
    print (maximum [V "a", V "b", V "c"], minimum [V "b", V "a", V "c"])
    let nan = dv 0.0 0.0
    print (minimum [1.0, nan, 2.0], minimum [nan, 1.0], maximum [1.0, nan, 2.0], maximum [nan, 1.0])
    print (maximum [nan], minimum [2.0, 1.0, nan], maximum [nan, nan, 3.0])
    print (maximum (Just (3 :: Int)), minimum (Just (P 4 "j")))
    print (maximum (Right 7 :: Either String Int), minimum (Right (P 8 "r") :: Either String P))
    print (maximum [(1 :: Int, "b"), (1, "a")], minimum [(1 :: Int, "b"), (1, "a")])
    print (maximum [(1 :: Int, P 1 "a"), (1, P 1 "b")], minimum [(1 :: Int, P 1 "a"), (1, P 1 "b")])
    print (maximum ["b", "ab", "b"], minimum ["b", "ab", "a"])
    print (maximum [True, False], minimum [True, False], maximum [LT, GT, EQ])
    print (maximum [1 .. 100000 :: Int], minimum [1 .. 100000 :: Int])
    print (maximum (map (\i -> P (i `mod` 7) (show i)) [1 .. 100000 :: Int]))
    print (minimum (map (\i -> P (i `mod` 7) (show i)) [1 .. 100000 :: Int]))
    print (max (P 1 "x") (P 1 "y"), min (P 1 "x") (P 1 "y"))
    print (foldr max 0 [3, 7, 2 :: Int], foldl1 max [3, 7, 2 :: Int], foldr1 min [3, 7, 2 :: Int])
    putStrLn (extremes (Bag [P 1 "a", P 1 "b", P 0 "c"]))
    putStrLn (extremes (Bag [M 1, M 2]))
    putStrLn (extremes (Bag [1.0, nan, 2.0]))
    putStrLn (extremes (Bag [(2 :: Int, "x"), (2, "y")]))
    catch (print (minimum (Nothing :: Maybe Int))) (\e -> putStrLn ("caught: " <> e))
    catch (print (maximum (Left "no" :: Either String Int))) (\e -> putStrLn ("caught: " <> e))
