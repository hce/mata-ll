module Main where

-- T1 (2026-10 fresh-eyes review): a superclass method used under a
-- subclass constraint bypassed the dictionary in the dictionary-passing
-- fallback. The typechecker discharges `Eq a` from a declared `Ord a` by
-- entailment, which leaves no runtime evidence; the fallback took one
-- dictionary per DECLARED constraint, so `x == y` under `Ord a` reached
-- codegen raw — Lua's `==`, table identity — and `show x` under a class
-- with a Show superclass fell to the type-erased runtime show. Every
-- constraint list a dictionary convention is built from is now the
-- superclass closure of the declared context. Shapes: Eq under Ord past
-- the specialization limit (18 list levels) and at an existential type;
-- Show under a user class with a Show superclass; a two-level user chain
-- (`class Ord a => Sortable a`); two superclasses; a superclass that is
-- also declared explicitly, in both orders, and among more constraints;
-- the builtin containers' Eq; Functor/Applicative under Monad; the Eq step
-- inside a context instance's dictionary form (`instance Ord a => Ord
-- (Box a)` using `==`); the Integral chain (Real, Num, Ord, Eq, Enum)
-- and Fractional's Num, past the limit and at an existential type.

same :: Ord a => a -> a -> Bool
same x y = x == y

data W = W Int deriving (Show)
instance Eq W where
    (==) (W a) (W b) = (a `mod` 2) == (b `mod` 2)
instance Ord W where
    compare (W a) (W b) = compare a b

data Some = forall a. Ord a => Some a

selfSame :: Some -> Bool
selfSame (Some x) = same x x

data V = V Int deriving (Show, Eq, Ord)

class Show a => Named a where
    name :: a -> String

instance Named V where
    name (V n) = "v" <> show n

class Ord a => Sortable a where
    key :: a -> Int

instance Sortable V where
    key (V n) = n

class (Show a, Eq a) => Tagged a where
    tag :: a -> String

instance Tagged V where
    tag _ = "V"

describe :: Named a => Int -> d -> a -> String
describe 0 _ x = name x <> "=" <> show x
describe n z x = describe (n - 1) [z] x

sortish :: Sortable a => Int -> d -> a -> a -> String
sortish 0 _ x y = show (x == y) <> show (x < y) <> show (compare x y) <> show (key x + key y) <> show (x /= y) <> show (key (max x y))
sortish n z x y = sortish (n - 1) [z] x y

tagged :: Tagged a => Int -> d -> a -> a -> String
tagged 0 _ x y = tag x <> show x <> show (x == y)
tagged n z x y = tagged (n - 1) [z] x y

dup1 :: (Eq a, Ord a) => Int -> d -> a -> a -> String
dup1 0 _ x y = show (x == y) <> show (x <= y)
dup1 n z x y = dup1 (n - 1) [z] x y

dup2 :: (Ord a, Eq a) => Int -> d -> a -> a -> String
dup2 0 _ x y = show (x == y) <> show (x <= y)
dup2 n z x y = dup2 (n - 1) [z] x y

dup3 :: (Ord a, Eq a, Show a, Sortable a) => Int -> d -> a -> a -> String
dup3 0 _ x y = show (x == y) <> show (x <= y) <> show x <> show (key y)
dup3 n z x y = dup3 (n - 1) [z] x y

twice :: Monad m => Int -> d -> m Int -> m Int
twice 0 _ m = do
    x <- m
    y <- fmap (+ 1) (pure x)
    return (x + y)
twice n z m = twice (n - 1) [z] m

data Box a = Box a deriving (Show)
instance Eq a => Eq (Box a) where
    (==) (Box x) (Box y) = x == y
instance Ord a => Ord (Box a) where
    compare (Box x) (Box y) = if x == y then EQ else compare x y

cmpBox :: Ord a => Int -> d -> Box a -> Box a -> String
cmpBox 0 _ x y = show (compare x y)
cmpBox n z x y = cmpBox (n - 1) [z] x y

viaSome :: Some -> String
viaSome (Some x) = cmpBox 0 () (Box x) (Box x)

halve :: (Show a, Integral a) => Int -> d -> a -> String
halve 0 _ x = show (x `div` 2) <> show (x == 4) <> show (x < 5) <> show (x + 1) <> show (fromIntegral (toInteger x) + (1 :: Int))
halve n z x = halve (n - 1) [z] x

frac :: Fractional a => Int -> d -> a -> a
frac 0 _ x = x / 2 + 1
frac n z x = frac (n - 1) [z] x

data SomeI = forall a. (Show a, Integral a) => SomeI a

unI :: SomeI -> String
unI (SomeI x) = show (x `mod` 3) <> show (x == 7)

sameLevels :: IO ()
sameLevels = do
    print (same (1 :: Int) 1)
    print (same [1 :: Int] [1])
    print (same [[1 :: Int]] [[1]])
    print (same [[[1 :: Int]]] [[[1]]])
    print (same [[[[1 :: Int]]]] [[[[1]]]])
    print (same [[[[[1 :: Int]]]]] [[[[[1]]]]])
    print (same [[[[[[1 :: Int]]]]]] [[[[[[1]]]]]])
    print (same [[[[[[[1 :: Int]]]]]]] [[[[[[[1]]]]]]])
    print (same [[[[[[[[1 :: Int]]]]]]]] [[[[[[[[1]]]]]]]])
    print (same [[[[[[[[[1 :: Int]]]]]]]]] [[[[[[[[[1]]]]]]]]])
    print (same [[[[[[[[[[1 :: Int]]]]]]]]]] [[[[[[[[[[1]]]]]]]]]])
    print (same [[[[[[[[[[[1 :: Int]]]]]]]]]]] [[[[[[[[[[[1]]]]]]]]]]])
    print (same [[[[[[[[[[[[1 :: Int]]]]]]]]]]]] [[[[[[[[[[[[1]]]]]]]]]]]])
    print (same [[[[[[[[[[[[[1 :: Int]]]]]]]]]]]]] [[[[[[[[[[[[[1]]]]]]]]]]]]])
    print (same [[[[[[[[[[[[[[1 :: Int]]]]]]]]]]]]]] [[[[[[[[[[[[[[1]]]]]]]]]]]]]])
    print (same [[[[[[[[[[[[[[[1 :: Int]]]]]]]]]]]]]]] [[[[[[[[[[[[[[[1]]]]]]]]]]]]]]])
    print (same [[[[[[[[[[[[[[[[1 :: Int]]]]]]]]]]]]]]]] [[[[[[[[[[[[[[[[1]]]]]]]]]]]]]]]])
    print (same [[[[[[[[[[[[[[[[[1 :: Int]]]]]]]]]]]]]]]]] [[[[[[[[[[[[[[[[[1]]]]]]]]]]]]]]]]])
    print (same (W 2) (W 4))
    print (selfSame (Some (W 1)))

userClasses :: IO ()
userClasses = do
    putStrLn (describe 20 () (V 7))
    putStrLn (sortish 20 () (V 1) (V 2))
    putStrLn (sortish 20 () (V 3) (V 3))
    putStrLn (tagged 20 () (V 1) (V 1))
    putStrLn (tagged 20 () (V 1) (V 2))
    putStrLn (dup1 20 () (V 1) (V 2))
    putStrLn (dup2 20 () (V 2) (V 2))
    putStrLn (dup3 20 () (V 5) (V 4))
    putStrLn (dup1 20 () (1 :: Int) 2)
    putStrLn (dup1 20 () [1 :: Int] [1])
    putStrLn (dup1 20 () (Just "a") Nothing)
    putStrLn (dup1 20 () (1 :: Int, "x") (1, "x"))
    putStrLn (dup1 20 () (Left 1 :: Either Int String) (Right "r"))

builtinChains :: IO ()
builtinChains = do
    print (twice 20 () (Just (3 :: Int)))
    print (twice 20 () [1 :: Int, 2])
    print (twice 20 () (Right 4 :: Either String Int))
    putStrLn (cmpBox 20 () (Box (W 1)) (Box (W 3)))
    putStrLn (cmpBox 20 () (Box (W 1)) (Box (W 2)))
    putStrLn (cmpBox 20 () (Box (1 :: Int)) (Box 2))
    putStrLn (viaSome (Some (W 5)))
    putStrLn (halve 20 () (4 :: Int))
    putStrLn (halve 20 () (9 :: Integer))
    print (frac 20 () (3.0 :: Number))
    putStrLn (unI (SomeI (7 :: Int)))

main :: IO ()
main = do
    sameLevels
    userClasses
    builtinChains

-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: True
-- expect: v7=V 7
-- expect: FalseTrueLT3True2
-- expect: TrueFalseEQ6False3
-- expect: VV 1True
-- expect: VV 1False
-- expect: FalseTrue
-- expect: TrueTrue
-- expect: FalseFalseV 54
-- expect: FalseTrue
-- expect: TrueTrue
-- expect: FalseFalse
-- expect: TrueTrue
-- expect: FalseTrue
-- expect: Just 7
-- expect: [3,5]
-- expect: Right 9
-- expect: EQ
-- expect: LT
-- expect: LT
-- expect: EQ
-- expect: 2TrueTrue55
-- expect: 4FalseFalse1010
-- expect: 2.5
-- expect: 1True
