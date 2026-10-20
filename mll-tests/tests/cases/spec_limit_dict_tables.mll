-- Regression (fresh-eyes review 2026-10, T13): a static dictionary is built
-- after the fixpoint, so it never names a method that later switched to
-- dictionary passing. `print` at 21 types trips the specialization cap
-- first; its call sites then build a `Show (Box T)` dictionary each, and
-- the 18th of those trips the cap of the instance method. The tables built
-- before that trip named the purged specializations, which the purge
-- rewrite renamed to the method's original — by then its dictionary form,
-- taking a context dictionary first — so every call through such a table
-- had its arguments shifted by one: "attempt to index a nil value" on the
-- first line. A ground type's dictionary is now a placeholder until the
-- monomorphizer's fixpoint is final, and decided once, in one place.
-- Covers: a written instance with a context (Show), Eq through a
-- dictionary-passing caller, Ord with its Eq superclass, a derived instance
-- (W21's dictionary form), a two-variable context (Pair), the instance at
-- three types with `print` at 21 (the specialized tables must stay right),
-- a dictionary-passing body using the instance at a ground type, and
-- nesting (Box in Box, Pair in Pair, Box in Tri).
module Main where

data Box a = Box a

instance Show a => Show (Box a) where
    showsPrec d (Box x) = showParen (d > 10) (showString "Box " . showsPrec 11 x)

instance Eq a => Eq (Box a) where
    (Box x) == (Box y) = x == y

instance Ord a => Ord (Box a) where
    compare (Box x) (Box y) = compare x y

data Tri a = Tri a deriving (Show, Eq)

data Pair a b = Pair a b

instance (Show a, Show b) => Show (Pair a b) where
    showsPrec d (Pair x y) = showParen (d > 10) (showString "Pair " . showsPrec 11 x . showString " " . showsPrec 11 y)

data Nest a = Leaf a | Nest (Nest [a])

same :: Eq a => a -> a -> Bool
same x y = x == y

lt :: Ord a => a -> a -> Bool
lt x y = x < y

cmp :: Ord a => a -> a -> Ordering
cmp x y = compare x y

deep :: Show a => Int -> Nest a -> String
deep n (Leaf x) = show x <> " " <> show (Box n)
deep n (Nest r) = deep (n + 1) r

main :: IO ()
main = do
    putStrLn (deep 0 (Nest (Leaf [1 :: Int])))
    print (Box (1 :: Int))
    print (Box (2.5 :: Number))
    print (Box "s")
    print (Box True)
    print (Box [1 :: Int])
    print (Box (Just (1 :: Int)))
    print (Box ((1 :: Int), (2 :: Int)))
    print (Box ["a"])
    print (Box (Just "b"))
    print (Box ((1 :: Int), "c"))
    print (Box [True])
    print (Box (Just False))
    print (Box (True, (3 :: Int)))
    print (Box [[4 :: Int]])
    print (Box (Just (Just (5 :: Int))))
    print (Box ((6 :: Int), (7 :: Int), (8 :: Int)))
    print (Box [9.5 :: Number])
    print (Box (Just [1 :: Int]))
    print (Box [(1 :: Int, True)])
    print (Box (Just (2.5 :: Number)))
    print (Box ())
    print (Box (Box (1 :: Int)))
    print (Tri (1 :: Int))
    print (Tri (2.5 :: Number))
    print (Tri "s")
    print (Tri True)
    print (Tri [1 :: Int])
    print (Tri (Just (1 :: Int)))
    print (Tri ((1 :: Int), (2 :: Int)))
    print (Tri ["a"])
    print (Tri (Just "b"))
    print (Tri ((1 :: Int), "c"))
    print (Tri [True])
    print (Tri (Just False))
    print (Tri (True, (3 :: Int)))
    print (Tri [[4 :: Int]])
    print (Tri (Just (Just (5 :: Int))))
    print (Tri ((6 :: Int), (7 :: Int), (8 :: Int)))
    print (Tri [9.5 :: Number])
    print (Tri (Just [1 :: Int]))
    print (Tri [(1 :: Int, True)])
    print (Tri (Just (2.5 :: Number)))
    print (Tri ())
    print (Tri (Box (Tri (1 :: Int))))
    print (Pair (1 :: Int) "a")
    print (Pair (2.5 :: Number) True)
    print (Pair "s" (3 :: Int))
    print (Pair True [1 :: Int])
    print (Pair [1 :: Int] (Just "b"))
    print (Pair (Just (1 :: Int)) ())
    print (Pair ((1 :: Int), (2 :: Int)) "c")
    print (Pair ["a"] (4 :: Int))
    print (Pair (Just "b") (5.5 :: Number))
    print (Pair ((1 :: Int), "c") False)
    print (Pair [True] (Just (6 :: Int)))
    print (Pair (Just False) [7.5 :: Number])
    print (Pair (True, (3 :: Int)) "d")
    print (Pair [[4 :: Int]] [[5 :: Int]])
    print (Pair (Just (Just (5 :: Int))) (Just (Just "e")))
    print (Pair ((6 :: Int), (7 :: Int), (8 :: Int)) (9 :: Int))
    print (Pair [9.5 :: Number] [True])
    print (Pair (Just [1 :: Int]) (Just [2 :: Int]))
    print (Pair [(1 :: Int, True)] [(2 :: Int, "f")])
    print (Pair (Just (2.5 :: Number)) "g")
    print (Pair () ())
    print (Pair (Pair (1 :: Int) (2 :: Int)) (Pair "h" True))
    print (Just (Pair (1 :: Int) (Box "i")))
    print (same (Box (1 :: Int)) (Box 1))
    print (same (Box (2.5 :: Number)) (Box 2.0))
    print (same (Box "s") (Box "s"))
    print (same (Box True) (Box False))
    print (same (Box [1 :: Int]) (Box [1]))
    print (same (Box (Just (1 :: Int))) (Box Nothing))
    print (same (Box ((1 :: Int), (2 :: Int))) (Box (1, 2)))
    print (same (Box ["a"]) (Box ["b"]))
    print (same (Box (Just "b")) (Box (Just "b")))
    print (same (Box ((1 :: Int), "c")) (Box (2, "c")))
    print (same (Box [True]) (Box [True]))
    print (same (Box (Just False)) (Box (Just True)))
    print (same (Box (True, (3 :: Int))) (Box (True, 3)))
    print (same (Box [[4 :: Int]]) (Box [[5]]))
    print (same (Box (Just (Just (5 :: Int)))) (Box (Just (Just 5))))
    print (same (Box ((6 :: Int), (7 :: Int), (8 :: Int))) (Box (6, 7, 9)))
    print (same (Box [9.5 :: Number]) (Box [9.5]))
    print (same (Box (Just [1 :: Int])) (Box (Just [2])))
    print (same (Box [(1 :: Int, True)]) (Box [(1, True)]))
    print (same (Box (Just (2.5 :: Number))) (Box (Just 2.5)))
    print (same (Box ()) (Box ()))
    print (same (Box (Box (1 :: Int))) (Box (Box 2)))
    print (same (Tri (Box (1 :: Int))) (Tri (Box 1)))
    print (lt (Box (1 :: Int)) (Box 2), cmp (Box (1 :: Int)) (Box 2))
    print (lt (Box "s") (Box "t"), cmp (Box "s") (Box "s"))
    print (lt (Box [1 :: Int]) (Box [1, 2]), cmp (Box [1 :: Int]) (Box [1, 2]))
    print (lt (Box (Just (1 :: Int))) (Box Nothing), cmp (Box (Just (1 :: Int))) (Box Nothing))
    print (lt (Box (Box (1 :: Int))) (Box (Box 2)), cmp (Box (Box (1 :: Int))) (Box (Box 2)))
    print (max (Box (1 :: Int)) (Box 2) == Box 2, min (Box "a") (Box "b") == Box "a")
    putStrLn (deep 0 (Nest (Nest (Leaf [["x"]]))))
