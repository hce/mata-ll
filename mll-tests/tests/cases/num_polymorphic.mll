-- Numeric typeclass polymorphism: Num / Fractional functions usable at both
-- Integer and Number, plus numeric-literal defaulting.

-- Polymorphic over any Num: works at Integer AND Number.
sumN :: Num a => [a] -> a
sumN []     = fromInteger 0
sumN (x:xs) = x + sumN xs

double :: Num a => a -> a
double x = x + x

-- Polymorphic over Fractional (Number, not Integer).
average :: Fractional a => a -> a -> a
average x y = (x + y) / fromInteger 2

-- signum/abs/negate over any Num.
sign3 :: Num a => a -> a
sign3 x = signum x + signum x + signum x

approx :: Number -> Number -> Bool
approx a b = let d = a - b in (if d < 0.0 then negate d else d) < 0.001

main :: IO ()
main = do
    -- Same polymorphic function at Integer:
    assert (sumN [1, 2, 3, 4 :: Integer] == 10) "sumN Integer"
    assert (double (21 :: Integer) == 42) "double Integer"
    -- …and at Number:
    assert (approx (sumN [1.5, 2.5, 4.0]) 8.0) "sumN Number"
    assert (approx (double 1.5) 3.0) "double Number"
    assert (approx (average 2.0 6.0) 4.0) "average Number"
    -- negate / abs / signum:
    assert (negate (5 :: Integer) == (-5)) "negate Integer"
    assert (abs (-7 :: Integer) == 7) "abs Integer"
    assert (sign3 (-9 :: Integer) == (-3)) "signum Integer"
    assert (approx (abs (-2.5)) 2.5) "abs Number"
    -- Defaulting: an otherwise-unconstrained literal is Integer.
    assert (show (2 + 3) == "5") "default sum to Integer"
    assert (show (2 * 3 + 1) == "7") "default expr to Integer"
    -- A literal forced into a Number context defaults to Number there.
    assert (approx (1 + 0.5) 1.5) "int literal used at Number"
    putStrLn "num_polymorphic ok"
