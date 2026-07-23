-- Integral class methods with GHC's exact negative-number semantics.
--   div/mod  : floor division; remainder (mod) takes the DIVISOR's sign.
--   quot/rem : truncation toward zero; remainder (rem) takes the DIVIDEND's sign.
-- quotRem/divMod return both quotient and remainder as a pair.

main :: IO ()
main = do
    -- Positive operands: all four agree.
    assert (17 `div`  5 == 3)  "div pos"
    assert (17 `mod`  5 == 2)  "mod pos"
    assert (17 `quot` 5 == 3)  "quot pos"
    assert (17 `rem`  5 == 2)  "rem pos"

    -- Negative dividend (the case where div/mod and quot/rem diverge).
    assert ((-17) `div`  5 == (-4)) "div neg dividend (floor)"
    assert ((-17) `mod`  5 == 3)    "mod neg dividend (sign of divisor)"
    assert ((-17) `quot` 5 == (-3)) "quot neg dividend (toward zero)"
    assert ((-17) `rem`  5 == (-2)) "rem neg dividend (sign of dividend)"

    -- Negative divisor.
    assert (17 `div`  (-5) == (-4)) "div neg divisor (floor)"
    assert (17 `mod`  (-5) == (-3)) "mod neg divisor (sign of divisor)"
    assert (17 `quot` (-5) == (-3)) "quot neg divisor (toward zero)"
    assert (17 `rem`  (-5) == 2)    "rem neg divisor (sign of dividend)"

    -- Both negative.
    assert ((-17) `div`  (-5) == 3)    "div both neg"
    assert ((-17) `mod`  (-5) == (-2)) "mod both neg"
    assert ((-17) `quot` (-5) == 3)    "quot both neg"
    assert ((-17) `rem`  (-5) == (-2)) "rem both neg"

    -- quotRem / divMod return (quotient, remainder).
    let (q1, r1) = quotRem (-17) 5
    assert (q1 == (-3) && r1 == (-2)) "quotRem neg"
    let (q2, r2) = divMod (-17) 5
    assert (q2 == (-4) && r2 == 3) "divMod neg"

    -- The identities GHC guarantees.
    assert ((-17) == 5 * ((-17) `quot` 5) + ((-17) `rem` 5)) "quot/rem identity"
    assert ((-17) == 5 * ((-17) `div`  5) + ((-17) `mod` 5)) "div/mod identity"

    -- fromInteger is the identity at Int (no bignum type to convert through).
    assert (fromInteger 42 == (42 :: Int)) "fromInteger Int"


    putStrLn "integral_semantics ok"
