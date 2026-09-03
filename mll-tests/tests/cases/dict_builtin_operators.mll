-- Dictionary passing at the BUILTIN numeric types. A class-constrained
-- function used at more than 16 distinct types stops being specialized and
-- is compiled once with dictionary passing. The Int/Number instances map
-- `+`/`-`/`*` (and `div`/`mod`/`quot`/`rem` at Int, `/` at Number) to
-- THEMSELVES — applied uses are inlined as Lua operators and there is no
-- runtime function under those names — so the constructed dictionaries
-- must carry a real callable for them. They once referenced a global named
-- after the operator: `attempt to call a nil value (field '_usr_plus_')`
-- at the first use past the limit (an isolated-review find). Sixteen
-- newtypes exhaust the limit first; the builtin uses come after them.

newtype N1 = N1 Int deriving (Show, Eq)
instance Num N1 where
    (+) (N1 a) (N1 b) = N1 (a + b)
    (-) (N1 a) (N1 b) = N1 (a - b)
    (*) (N1 a) (N1 b) = N1 (a * b)
    negate (N1 a) = N1 (negate a)
    abs (N1 a) = N1 (abs a)
    signum (N1 a) = N1 (signum a)
    fromInteger i = N1 (fromInteger i)

newtype N2 = N2 Int deriving (Show, Eq)
instance Num N2 where
    (+) (N2 a) (N2 b) = N2 (a + b)
    (-) (N2 a) (N2 b) = N2 (a - b)
    (*) (N2 a) (N2 b) = N2 (a * b)
    negate (N2 a) = N2 (negate a)
    abs (N2 a) = N2 (abs a)
    signum (N2 a) = N2 (signum a)
    fromInteger i = N2 (fromInteger i)

newtype N3 = N3 Int deriving (Show, Eq)
instance Num N3 where
    (+) (N3 a) (N3 b) = N3 (a + b)
    (-) (N3 a) (N3 b) = N3 (a - b)
    (*) (N3 a) (N3 b) = N3 (a * b)
    negate (N3 a) = N3 (negate a)
    abs (N3 a) = N3 (abs a)
    signum (N3 a) = N3 (signum a)
    fromInteger i = N3 (fromInteger i)

newtype N4 = N4 Int deriving (Show, Eq)
instance Num N4 where
    (+) (N4 a) (N4 b) = N4 (a + b)
    (-) (N4 a) (N4 b) = N4 (a - b)
    (*) (N4 a) (N4 b) = N4 (a * b)
    negate (N4 a) = N4 (negate a)
    abs (N4 a) = N4 (abs a)
    signum (N4 a) = N4 (signum a)
    fromInteger i = N4 (fromInteger i)

newtype N5 = N5 Int deriving (Show, Eq)
instance Num N5 where
    (+) (N5 a) (N5 b) = N5 (a + b)
    (-) (N5 a) (N5 b) = N5 (a - b)
    (*) (N5 a) (N5 b) = N5 (a * b)
    negate (N5 a) = N5 (negate a)
    abs (N5 a) = N5 (abs a)
    signum (N5 a) = N5 (signum a)
    fromInteger i = N5 (fromInteger i)

newtype N6 = N6 Int deriving (Show, Eq)
instance Num N6 where
    (+) (N6 a) (N6 b) = N6 (a + b)
    (-) (N6 a) (N6 b) = N6 (a - b)
    (*) (N6 a) (N6 b) = N6 (a * b)
    negate (N6 a) = N6 (negate a)
    abs (N6 a) = N6 (abs a)
    signum (N6 a) = N6 (signum a)
    fromInteger i = N6 (fromInteger i)

newtype N7 = N7 Int deriving (Show, Eq)
instance Num N7 where
    (+) (N7 a) (N7 b) = N7 (a + b)
    (-) (N7 a) (N7 b) = N7 (a - b)
    (*) (N7 a) (N7 b) = N7 (a * b)
    negate (N7 a) = N7 (negate a)
    abs (N7 a) = N7 (abs a)
    signum (N7 a) = N7 (signum a)
    fromInteger i = N7 (fromInteger i)

newtype N8 = N8 Int deriving (Show, Eq)
instance Num N8 where
    (+) (N8 a) (N8 b) = N8 (a + b)
    (-) (N8 a) (N8 b) = N8 (a - b)
    (*) (N8 a) (N8 b) = N8 (a * b)
    negate (N8 a) = N8 (negate a)
    abs (N8 a) = N8 (abs a)
    signum (N8 a) = N8 (signum a)
    fromInteger i = N8 (fromInteger i)

newtype N9 = N9 Int deriving (Show, Eq)
instance Num N9 where
    (+) (N9 a) (N9 b) = N9 (a + b)
    (-) (N9 a) (N9 b) = N9 (a - b)
    (*) (N9 a) (N9 b) = N9 (a * b)
    negate (N9 a) = N9 (negate a)
    abs (N9 a) = N9 (abs a)
    signum (N9 a) = N9 (signum a)
    fromInteger i = N9 (fromInteger i)

newtype N10 = N10 Int deriving (Show, Eq)
instance Num N10 where
    (+) (N10 a) (N10 b) = N10 (a + b)
    (-) (N10 a) (N10 b) = N10 (a - b)
    (*) (N10 a) (N10 b) = N10 (a * b)
    negate (N10 a) = N10 (negate a)
    abs (N10 a) = N10 (abs a)
    signum (N10 a) = N10 (signum a)
    fromInteger i = N10 (fromInteger i)

newtype N11 = N11 Int deriving (Show, Eq)
instance Num N11 where
    (+) (N11 a) (N11 b) = N11 (a + b)
    (-) (N11 a) (N11 b) = N11 (a - b)
    (*) (N11 a) (N11 b) = N11 (a * b)
    negate (N11 a) = N11 (negate a)
    abs (N11 a) = N11 (abs a)
    signum (N11 a) = N11 (signum a)
    fromInteger i = N11 (fromInteger i)

newtype N12 = N12 Int deriving (Show, Eq)
instance Num N12 where
    (+) (N12 a) (N12 b) = N12 (a + b)
    (-) (N12 a) (N12 b) = N12 (a - b)
    (*) (N12 a) (N12 b) = N12 (a * b)
    negate (N12 a) = N12 (negate a)
    abs (N12 a) = N12 (abs a)
    signum (N12 a) = N12 (signum a)
    fromInteger i = N12 (fromInteger i)

newtype N13 = N13 Int deriving (Show, Eq)
instance Num N13 where
    (+) (N13 a) (N13 b) = N13 (a + b)
    (-) (N13 a) (N13 b) = N13 (a - b)
    (*) (N13 a) (N13 b) = N13 (a * b)
    negate (N13 a) = N13 (negate a)
    abs (N13 a) = N13 (abs a)
    signum (N13 a) = N13 (signum a)
    fromInteger i = N13 (fromInteger i)

newtype N14 = N14 Int deriving (Show, Eq)
instance Num N14 where
    (+) (N14 a) (N14 b) = N14 (a + b)
    (-) (N14 a) (N14 b) = N14 (a - b)
    (*) (N14 a) (N14 b) = N14 (a * b)
    negate (N14 a) = N14 (negate a)
    abs (N14 a) = N14 (abs a)
    signum (N14 a) = N14 (signum a)
    fromInteger i = N14 (fromInteger i)

newtype N15 = N15 Int deriving (Show, Eq)
instance Num N15 where
    (+) (N15 a) (N15 b) = N15 (a + b)
    (-) (N15 a) (N15 b) = N15 (a - b)
    (*) (N15 a) (N15 b) = N15 (a * b)
    negate (N15 a) = N15 (negate a)
    abs (N15 a) = N15 (abs a)
    signum (N15 a) = N15 (signum a)
    fromInteger i = N15 (fromInteger i)

newtype N16 = N16 Int deriving (Show, Eq)
instance Num N16 where
    (+) (N16 a) (N16 b) = N16 (a + b)
    (-) (N16 a) (N16 b) = N16 (a - b)
    (*) (N16 a) (N16 b) = N16 (a * b)
    negate (N16 a) = N16 (negate a)
    abs (N16 a) = N16 (abs a)
    signum (N16 a) = N16 (signum a)
    fromInteger i = N16 (fromInteger i)

data D = D Int deriving (Show, Eq)
instance Num D where
    (+) (D a) (D b) = D (a + b)
    (-) (D a) (D b) = D (a - b)
    (*) (D a) (D b) = D (a * b)
    negate (D a) = D (negate a)
    abs (D a) = D (abs a)
    signum (D a) = D (signum a)
    fromInteger i = D (fromInteger i)

-- The literal `2` is an overloaded literal: `fromInteger 2` through the
-- dictionary (a raw machine `2` reaching the Integer instance's `*` was
-- "attempt to index a number value").
arith :: Num a => a -> a -> a
arith x y = (x + y) * 2 - negate y + abs (x - y) * signum x + fromInteger 1

-- Literals under a SUBCLASS constraint (the Num dictionary is reached
-- through Integral / Fractional).
scaled :: Integral a => a -> a
scaled x = x * 2 + 1

frac :: Fractional a => a -> a
frac x = x * 2.5 - 0.5

halves :: Integral a => a -> (a, a, a, a)
halves x = (x `div` 3, x `mod` 3, x `quot` (-3), x `rem` (-3))

ratio :: Fractional a => a -> a -> a
ratio x y = x / y + recip y

main :: IO ()
main = do
    print (arith (N1 1) (N1 2))
    print (arith (N2 2) (N2 2))
    print (arith (N3 3) (N3 2))
    print (arith (N4 4) (N4 2))
    print (arith (N5 5) (N5 2))
    print (arith (N6 6) (N6 2))
    print (arith (N7 7) (N7 2))
    print (arith (N8 8) (N8 2))
    print (arith (N9 9) (N9 2))
    print (arith (N10 10) (N10 2))
    print (arith (N11 11) (N11 2))
    print (arith (N12 12) (N12 2))
    print (arith (N13 13) (N13 2))
    print (arith (N14 14) (N14 2))
    print (arith (N15 15) (N15 2))
    print (arith (N16 16) (N16 2))
    -- The 17th and later types: dictionary passing.
    print (arith (10 :: Int) 3)
    print (arith (-10 :: Int) 3)
    print (arith (2.5 :: Number) 1.5)
    print (arith (10 :: Integer) 3)
    print (arith (D 1) (D 2))
    print (scaled (10 :: Int), scaled (10 :: Integer))
    print (frac (2.0 :: Number))
    print (halves (10 :: Int))
    print (halves (-10 :: Int))
    print (halves (10 :: Integer))
    print (ratio (3.0 :: Number) 4.0)
    -- First-class operators drawn from a dictionary.
    print (foldr (+) (0 :: Int) [1, 2, 3])
    print (zipWith (*) [1.5 :: Number, 2] [2, 4])
