-- Numeric typeclass polymorphism: Num / Fractional functions usable at both
-- Int and Number, plus numeric-literal defaulting.

-- Polymorphic over any Num: works at Int AND Number.
sumN :: Num a => [a] -> a
sumN []     = fromInteger 0
sumN (x:xs) = x + sumN xs

double :: Num a => a -> a
double x = x + x

-- Polymorphic over Fractional (Number, not Int).
average :: Fractional a => a -> a -> a
average x y = (x + y) / fromInteger 2

-- signum/abs/negate over any Num.
sign3 :: Num a => a -> a
sign3 x = signum x + signum x + signum x

approx :: Number -> Number -> Bool
approx a b = let d = a - b in (if d < 0.0 then negate d else d) < 0.001

main :: IO ()
main = do
    -- Same polymorphic function at Int:
    assert (sumN [1, 2, 3, 4 :: Int] == 10) "sumN Int"
    assert (double (21 :: Int) == 42) "double Int"
    -- …and at Number:
    assert (approx (sumN [1.5, 2.5, 4.0]) 8.0) "sumN Number"
    assert (approx (double 1.5) 3.0) "double Number"
    assert (approx (average 2.0 6.0) 4.0) "average Number"
    -- negate / abs / signum:
    assert (negate (5 :: Int) == (-5)) "negate Int"
    assert (abs (-7 :: Int) == 7) "abs Int"
    assert (sign3 (-9 :: Int) == (-3)) "signum Int"
    assert (approx (abs (-2.5)) 2.5) "abs Number"
    -- Defaulting: an otherwise-unconstrained literal is Int.
    assert (show (2 + 3) == "5") "default sum to Int"
    assert (show (2 * 3 + 1) == "7") "default expr to Int"
    -- A literal forced into a Number context defaults to Number there.
    assert (approx (1 + 0.5) 1.5) "int literal used at Number"
    -- Scientific-notation literals (Haskell 2010 exponents).
    assert (approx 1.0e-2 0.01) "exponent: fractional mantissa, negative exp"
    assert (approx 1e5 100000.0) "exponent: bare mantissa is Fractional"
    assert (approx 2.5E+3 2500.0) "exponent: uppercase E, explicit + sign"
    assert (approx 6.022e23 (602.2 * 1e21)) "exponent: large magnitude"
    -- `show` now emits exponent notation that the lexer reads back (round-trip).
    assert (show (1.2345678e7 :: Number) == "1.2345678e7") "exponent: show emits e-notation"
    assert ((1.2345678e7 :: Number) == read (show (1.2345678e7 :: Number))) "exponent: read . show = id"
    putStrLn "num_polymorphic ok"
