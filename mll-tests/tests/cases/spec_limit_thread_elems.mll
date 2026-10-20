-- Regression (fresh-eyes review 2026-10, T13 family): a structural thread —
-- a list, Maybe or tuple equality, show or compare — carries its element
-- function by NAME, which no call-site rewrite reaches. When the element's
-- instance method trips the specialization cap, that name was either a
-- purged specialization renamed to the method's original (now a dictionary
-- form taking a context dictionary first) or, resolved after the trip, the
-- original itself; either way the runtime helper handed the element where
-- the dictionary was expected: "attempt to index a nil value" at
-- `[Box 1] == [Box 1]`. A dictionary-passing method at a ground type now
-- resolves to a stub of that type — the name its specialization had or
-- would have had, which passes the dictionaries — and a purged copy keeps
-- its name as that stub. Covers: list, Maybe and tuple Eq over the instance
-- at 21 element types (the trip happens inside the list uses), the same
-- threads resolved AFTER the trip (the instance tripped by plain uses
-- first), list/Maybe show, list compare, and a polymorphic-recursion
-- ladder whose copies are purged while under generation.
module Main where

data Box a = Box a

instance Show a => Show (Box a) where
    showsPrec d (Box x) = showParen (d > 10) (showString "Box " . showsPrec 11 x)

instance Eq a => Eq (Box a) where
    (Box x) == (Box y) = x == y

instance Ord a => Ord (Box a) where
    compare (Box x) (Box y) = compare x y

data Tag a = Tag a

instance Eq a => Eq (Tag a) where
    (Tag x) == (Tag y) = x == y

data Nest a = Leaf a | Nest (Nest [a])

deep :: Show a => Int -> Nest a -> String
deep n (Leaf x) = show [Box x] <> " " <> show (Just (Box n))
deep n (Nest r) = deep (n + 1) r

main :: IO ()
main = do
    print ([Box (1 :: Int)] == [Box 1])
    print ([Box (2.5 :: Number)] == [Box 2.0])
    print ([Box "s"] == [Box "s"])
    print ([Box True] == [Box False])
    print ([Box [1 :: Int]] == [Box [1]])
    print ([Box (Just (1 :: Int))] == [Box Nothing])
    print ([Box ((1 :: Int), (2 :: Int))] == [Box (1, 2)])
    print ([Box ["a"]] == [Box ["b"]])
    print ([Box (Just "b")] == [Box (Just "b")])
    print ([Box ((1 :: Int), "c")] == [Box (2, "c")])
    print ([Box [True]] == [Box [True]])
    print ([Box (Just False)] == [Box (Just True)])
    print ([Box (True, (3 :: Int))] == [Box (True, 3)])
    print ([Box [[4 :: Int]]] == [Box [[5]]])
    print ([Box (Just (Just (5 :: Int)))] == [Box (Just (Just 5))])
    print ([Box ((6 :: Int), (7 :: Int), (8 :: Int))] == [Box (6, 7, 9)])
    print ([Box [9.5 :: Number]] == [Box [9.5]])
    print ([Box (Just [1 :: Int])] == [Box (Just [2])])
    print ([Box [(1 :: Int, True)]] == [Box [(1, True)]])
    print ([Box (Just (2.5 :: Number))] == [Box (Just 2.5)])
    print ([Box ()] == [Box ()])
    print (Just (Box (Box (1 :: Int))) == Just (Box (Box 2)))
    print ((Box (1 :: Int), Box "x") == (Box 1, Box "x"))
    print (Tag (1 :: Int) == Tag 1)
    print (Tag (2.5 :: Number) == Tag 2.0)
    print (Tag "s" == Tag "s")
    print (Tag True == Tag False)
    print (Tag [1 :: Int] == Tag [1])
    print (Tag (Just (1 :: Int)) == Tag Nothing)
    print (Tag ((1 :: Int), (2 :: Int)) == Tag (1, 2))
    print (Tag ["a"] == Tag ["b"])
    print (Tag (Just "b") == Tag (Just "b"))
    print (Tag ((1 :: Int), "c") == Tag (2, "c"))
    print (Tag [True] == Tag [True])
    print (Tag (Just False) == Tag (Just True))
    print (Tag (True, (3 :: Int)) == Tag (True, 3))
    print (Tag [[4 :: Int]] == Tag [[5]])
    print (Tag (Just (Just (5 :: Int))) == Tag (Just (Just 5)))
    print (Tag ((6 :: Int), (7 :: Int), (8 :: Int)) == Tag (6, 7, 9))
    print (Tag [9.5 :: Number] == Tag [9.5])
    print (Tag (Just [1 :: Int]) == Tag (Just [2]))
    print (Tag [(1 :: Int, True)] == Tag [(1, True)])
    print (Tag (Just (2.5 :: Number)) == Tag (Just 2.5))
    print (Tag () == Tag ())
    print ([Tag (1 :: Int)] == [Tag 1])
    print (Just (Tag "x") == Just (Tag "x"))
    print ([Tag (Tag (3 :: Int))] == [Tag (Tag 3)])
    print ((Tag (1 :: Int), Tag [True]) == (Tag 1, Tag [True]))
    putStrLn (deep 0 (Nest (Leaf [1 :: Int])))
    putStrLn (show [Box (1 :: Int), Box 2])
    putStrLn (show (Just (Box "j")))
    putStrLn (show [Just (Box (True, (1 :: Int)))])
    print (compare [Box (1 :: Int)] [Box 2], compare [Box "b"] [Box "a"], compare [Box [1 :: Int]] [Box [1]])
    print ([Box (1 :: Int)] < [Box 2], max [Box "a"] [Box "b"])
    putStrLn (deep 0 (Nest (Nest (Leaf [["x"]]))))
