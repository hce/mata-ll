-- A user numeric type with a hand-written Num instance. Numeric literals at
-- this type are desugared through the instance's `fromInteger`, and the
-- operators dispatch to the instance methods (NOT the built-in Lua operators).

newtype Z5 = Z5 Int

unZ5 :: Z5 -> Int
unZ5 (Z5 n) = n

instance Num Z5 where
    (+) (Z5 a) (Z5 b) = Z5 ((a + b) `mod` 5)
    (-) (Z5 a) (Z5 b) = Z5 ((a - b) `mod` 5)
    (*) (Z5 a) (Z5 b) = Z5 ((a * b) `mod` 5)
    negate (Z5 a)     = Z5 ((5 - a) `mod` 5)
    abs m             = m
    signum (Z5 0)     = Z5 0
    signum _          = Z5 1
    fromInteger n     = Z5 (n `mod` 5)

instance Eq Z5 where
    (==) (Z5 a) (Z5 b) = a == b

-- A polymorphic Num function instantiated at the user type.
triple :: Num a => a -> a
triple x = x + x + x

main :: IO ()
main = do
    -- Literals desugar via fromInteger: (3 :: Z5) = Z5 3, (7 :: Z5) = Z5 2.
    assert (unZ5 (3 :: Z5) == 3) "fromInteger small"
    assert (unZ5 (7 :: Z5) == 2) "fromInteger wraps"
    -- Operators use the instance, wrapping mod 5.
    assert (unZ5 (3 + 4 :: Z5) == 2) "user (+) wraps"
    assert (unZ5 (4 * 4 :: Z5) == 1) "user (*) wraps"
    assert (unZ5 (negate 2 :: Z5) == 3) "user negate"
    assert (unZ5 (1 - 3 :: Z5) == 3) "user (-) wraps"
    -- Polymorphic Num code specialised at the user type.
    assert (unZ5 (triple (4 :: Z5)) == 2) "polymorphic triple at Z5"
    -- Same polymorphic function at Int keeps native arithmetic.
    assert (triple (4 :: Int) == 12) "polymorphic triple at Int"
    putStrLn "num_user_instance ok"
