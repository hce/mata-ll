module Main where

-- T14 (2026-10 fresh-eyes review): an instance declaration's superclass
-- requirement is decided by instance resolution under the instance's
-- context, as a use site decides it (Haskell 2010 §4.3.2). The check used
-- to be a registry lookup keyed by the bare head, which refused every
-- instance over a list, tuple or Maybe head of a class with a superclass
-- (their Eq/Ord/Show instances are structural, never registered), and
-- could not tell `instance Eq a => Container [a]` from the context-free
-- spelling GHC rejects. Shapes: structural superclass instances (list,
-- Maybe, tuple) with their contexts; the superclass method called
-- through the subclass dictionary, specialized, past the specialization
-- limit and at an existential type; a two-superclass class; a context
-- class that is a superclass of the needed one (`Ord a` provides `Eq a`,
-- `Ord [a]` needs `Ord a`); a user instance with a context as the
-- witness (`Show (Tree a)`); a derived instance with a derived context
-- as the witness (`Eq (Pairs a b)`, `Ord (Box a)`); a builtin head; a
-- user-defined superclass whose witness instance is declared AFTER the
-- dependent instance; and the monad hierarchy at IO and Maybe.

class Eq a => Container a where
    size :: a -> Int

instance Eq a => Container [a] where
    size xs = length xs

instance Eq a => Container (Maybe a) where
    size Nothing = 0
    size (Just _) = 1

instance (Eq a, Eq b) => Container (a, b) where
    size _ = 2

instance Container Int where
    size n = n

newtype Pairs a b = Pairs [(a, Maybe b)] deriving (Eq, Show)

instance (Eq a, Eq b) => Container (Pairs a b) where
    size (Pairs ps) = length ps

class Show a => Pretty a where
    pretty :: a -> String

instance (Show a, Show b) => Pretty (a, b) where
    pretty (x, y) = show x <> " & " <> show y

instance Show a => Pretty [a] where
    pretty xs = "list " <> show xs

instance Show a => Pretty (Maybe a) where
    pretty Nothing = "none"
    pretty (Just x) = "some " <> show x

data Tree a = Leaf | Node (Tree a) a (Tree a)

instance Show a => Show (Tree a) where
    show Leaf = "."
    show (Node l x r) = "(" <> show l <> " " <> show x <> " " <> show r <> ")"

instance Show a => Pretty (Tree a) where
    pretty t = "tree " <> show t

class (Eq a, Show a) => Tagged a where
    tag :: a -> String

instance (Eq a, Show a) => Tagged [a] where
    tag xs = "L" <> show (length xs)

class Ord a => Ranked a where
    rank :: a -> Int

instance Ord a => Ranked [a] where
    rank xs = length xs

data Box a = Box a deriving (Eq, Ord, Show)

instance Ord a => Ranked (Box a) where
    rank (Box _) = 1

class Eq a => Sized a where
    measure :: a -> Int

instance Ord a => Sized [a] where
    measure xs = length xs

instance Sized Int where
    measure n = n

class Shape a where
    area :: a -> Int

class Shape a => Solid a where
    volume :: a -> Int

instance Shape a => Solid [a] where
    volume xs = sum (map area xs) * 2

instance Shape a => Shape [a] where
    area xs = sum (map area xs)

instance Solid Int where
    volume n = n * n * n

instance Shape Int where
    area n = n * n

class Monad m => Logger m where
    logMsg :: String -> m ()

instance Logger IO where
    logMsg s = putStrLn ("log: " <> s)

instance Logger Maybe where
    logMsg _ = Just ()

data SomeC = forall a. Container a => SomeC a

sameSize :: Container a => a -> a -> Bool
sameSize x y = x == y && size x == size y

prettyBoth :: Pretty a => a -> a -> String
prettyBoth x y = pretty x <> "/" <> show y

deep :: Container a => Int -> d -> a -> a -> String
deep 0 _ x y = show (sameSize x y) <> show (size x) <> show (x /= y)
deep n z x y = deep (n - 1) [z] x y

viaSome :: SomeC -> String
viaSome (SomeC x) = show (sameSize x x) <> show (size x)

describe :: Tagged a => a -> String
describe x = tag x <> show (x == x) <> show x

ranked :: Ranked a => a -> a -> String
ranked x y = show (rank x) <> show (x == y) <> show (x < y)

sized :: Sized a => a -> a -> String
sized x y = show (measure x) <> show (x == y)

contained :: Container a => a -> a -> String
contained x y = show (size x) <> show (x == y)

structural :: IO ()
structural = do
    print (size [1, 2, 3 :: Int])
    print (size (Just True))
    print (size (Nothing :: Maybe Int))
    print (sameSize [1, 2 :: Int] [1, 2])
    print (sameSize [1, 2 :: Int] [1, 3])
    print (sameSize (Just True) (Just True))
    print (sameSize (Just True) Nothing)
    print (sameSize (1 :: Int, True) (1, True))
    print (sameSize (1 :: Int, True) (2, True))
    putStrLn (prettyBoth (1 :: Int, True) (2, False))
    putStrLn (prettyBoth [1, 2 :: Int] [3])
    putStrLn (prettyBoth (Just "x") Nothing)

dictionaries :: IO ()
dictionaries = do
    putStrLn (deep 20 () [1, 2 :: Int] [1, 2])
    putStrLn (deep 20 () [1, 2 :: Int] [2, 1])
    putStrLn (deep 20 () (Just True) (Just True))
    putStrLn (deep 20 () (Just True) Nothing)
    putStrLn (deep 20 () (3 :: Int, "a") (3, "a"))
    putStrLn (deep 20 () (3 :: Int, "a") (3, "b"))
    putStrLn (deep 20 () [[1 :: Int], [2]] [[1], [2]])
    putStrLn (deep 20 () (Just [1 :: Int]) (Just [1]))
    putStrLn (viaSome (SomeC [1, 2, 3 :: Int]))
    putStrLn (viaSome (SomeC (Just "q")))
    putStrLn (viaSome (SomeC (1 :: Int, 2 :: Int)))
    putStrLn (viaSome (SomeC (7 :: Int)))
    putStrLn (viaSome (SomeC [Just (1 :: Int, True)]))

witnesses :: IO ()
witnesses = do
    putStrLn (describe [1, 2 :: Int])
    putStrLn (ranked [1 :: Int] [2])
    putStrLn (ranked (Box "a") (Box "a"))
    putStrLn (sized [True, False] [True, False])
    putStrLn (sized (5 :: Int) 6)
    putStrLn (pretty (Node Leaf (1 :: Int) Leaf))
    putStrLn (contained (Pairs [(1 :: Int, Just "a")]) (Pairs [(1, Nothing)]))
    print (volume [1, 2, 3 :: Int])
    print (area [[1, 2 :: Int], [3]])
    print (volume (3 :: Int))
    logMsg "hi"
    print (logMsg "x" :: Maybe ())

main :: IO ()
main = do
    structural
    dictionaries
    witnesses

-- expect: 3
-- expect: 1
-- expect: 0
-- expect: True
-- expect: False
-- expect: True
-- expect: False
-- expect: True
-- expect: False
-- expect: 1 & True/(2,False)
-- expect: list [1,2]/[3]
-- expect: some "x"/Nothing
-- expect: True2False
-- expect: False2True
-- expect: True1False
-- expect: False1True
-- expect: True2False
-- expect: False2True
-- expect: True2False
-- expect: True1False
-- expect: True3
-- expect: True1
-- expect: True2
-- expect: True7
-- expect: True1
-- expect: L2True[1,2]
-- expect: 1FalseTrue
-- expect: 1TrueFalse
-- expect: 2True
-- expect: 5False
-- expect: tree (. 1 .)
-- expect: 1False
-- expect: 28
-- expect: 14
-- expect: 27
-- expect: log: hi
-- expect: Just ()
