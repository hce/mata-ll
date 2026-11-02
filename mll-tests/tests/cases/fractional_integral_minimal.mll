-- Fractional's mutual defaults (`recip x = 1 / x`, `x / y = x * recip y`)
-- and Integral's defaults over quotRem (`quot`/`rem` project it, `divMod`
-- corrects it toward negative infinity, `div`/`mod` project that): an
-- instance that gives GHC's minimal definition — `/` or `recip` with
-- fromRational; quotRem with toInteger — is complete. Real has no method
-- an instance here must give (no Rational), so its instance is empty.

data Q = Q Int Int deriving (Show, Eq)

normQ :: Int -> Int -> Q
normQ n d = let g = gcd n d * signum d in Q (div n g) (div d g)

instance Num Q where
    (+) (Q a b) (Q c d) = normQ (a * d + c * b) (b * d)
    (*) (Q a b) (Q c d) = normQ (a * c) (b * d)
    negate (Q a b) = Q (negate a) b
    abs (Q a b) = Q (abs a) b
    signum (Q a _) = Q (signum a) 1
    fromInteger n = Q (fromInteger n) 1

instance Fractional Q where
    (/) (Q a b) (Q c d) = normQ (a * d) (b * c)
    fromRational _ = error "fromRational Q"

data R = R Int Int deriving (Show, Eq)

instance Num R where
    (+) (R a b) (R c d) = R (a * d + c * b) (b * d)
    (*) (R a b) (R c d) = R (a * c) (b * d)
    negate (R a b) = R (negate a) b
    abs (R a b) = R (abs a) b
    signum (R a _) = R (signum a) 1
    fromInteger n = R (fromInteger n) 1

instance Fractional R where
    recip (R a b) = R b a
    fromRational _ = error "fromRational R"

newtype I = I Int deriving (Show, Eq, Ord)

instance Num I where
    (+) (I a) (I b) = I (a + b)
    (*) (I a) (I b) = I (a * b)
    negate (I a) = I (negate a)
    abs (I a) = I (abs a)
    signum (I a) = I (signum a)
    fromInteger n = I (fromInteger n)

instance Real I where

instance Enum I where
    toEnum n = I n
    fromEnum (I n) = n

instance Integral I where
    quotRem (I a) (I b) = (I (quot a b), I (rem a b))
    toInteger (I n) = toInteger n

main :: IO ()
main = do
    print (Q 5 2 / Q 1 2, recip (Q 2 3), Q 1 3 / Q 1 3, recip (Q (-4) 1))
    print (R 5 2 / R 1 2, recip (R 2 3), R 1 3 / R 1 3)
    print [(quot a b, rem a b, div a b, mod a b) | a <- [I 7, I (-7)], b <- [I 2, I (-2)]]
    print [(quotRem a b, divMod a b) | a <- [I 7, I (-7)], b <- [I 2, I (-2)]]
    print (even (I 4), odd (I 4), toInteger (I 12) * 3, fromIntegral (I 7) + (1 :: Int))
    print (gcd (I 12) (I 18), lcm (I 4) (I 6), I 7 `div` I 2 + I 1)
