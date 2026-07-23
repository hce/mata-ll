-- Regression: the constant folder and the runtime must compute the SAME
-- div/mod. Both follow Haskell's FLOOR semantics: `div` rounds the quotient
-- toward negative infinity, `mod` takes the sign of the DIVISOR.
--
-- Before the fix the folder used Rust's div_euclid/rem_euclid (Euclidean
-- semantics, mod always non-negative), so a constant `7 `div` (-2)` folded
-- to -3 while the identical expression computed at runtime gave -4 — one
-- expression, two answers, depending on whether the operands were literals.
--
-- Each assertion pairs a literal expression (folded at compile time) with
-- the same computation routed through a function on runtime values (opaque
-- to the folder), across every sign combination.

d :: Int -> Int -> Int
d a b = a `div` b

m :: Int -> Int -> Int
m a b = a `mod` b

main :: IO ()
main = do
    -- positive / negative
    assert ((7 `div` (-2)) == -4) "folded 7 div -2 is floor (-4)"
    assert ((7 `mod` (-2)) == -1) "folded 7 mod -2 has divisor sign (-1)"
    assert (d 7 (-2) == (7 `div` (-2))) "runtime div agrees with folded, +/-"
    assert (m 7 (-2) == (7 `mod` (-2))) "runtime mod agrees with folded, +/-"

    -- negative / positive
    assert (((-7) `div` 2) == -4) "folded -7 div 2 is floor (-4)"
    assert (((-7) `mod` 2) == 1) "folded -7 mod 2 has divisor sign (1)"
    assert (d (-7) 2 == ((-7) `div` 2)) "runtime div agrees with folded, -/+"
    assert (m (-7) 2 == ((-7) `mod` 2)) "runtime mod agrees with folded, -/+"

    -- negative / negative
    assert (((-7) `div` (-2)) == 3) "folded -7 div -2 is 3"
    assert (((-7) `mod` (-2)) == -1) "folded -7 mod -2 is -1"
    assert (d (-7) (-2) == ((-7) `div` (-2))) "runtime div agrees with folded, -/-"
    assert (m (-7) (-2) == ((-7) `mod` (-2))) "runtime mod agrees with folded, -/-"

    -- positive / positive (unchanged, but pinned)
    assert ((7 `div` 2) == 3) "folded 7 div 2 is 3"
    assert ((7 `mod` 2) == 1) "folded 7 mod 2 is 1"
    assert (d 7 2 == (7 `div` 2)) "runtime div agrees with folded, +/+"
    assert (m 7 2 == (7 `mod` 2)) "runtime mod agrees with folded, +/+"

    -- exact multiples: no floor adjustment may fire when the remainder is 0
    assert ((6 `div` (-2)) == -3 && d 6 (-2) == -3) "exact multiple, negative divisor"
    assert ((6 `mod` (-2)) == 0 && m 6 (-2) == 0) "zero remainder stays zero"

    -- the div/mod law: a == b * (a div b) + (a mod b), all sign combinations
    assert (7 == (-2) * d 7 (-2) + m 7 (-2)) "div/mod law +/-"
    assert ((-7) == 2 * d (-7) 2 + m (-7) 2) "div/mod law -/+"
    assert ((-7) == (-2) * d (-7) (-2) + m (-7) (-2)) "div/mod law -/-"

    putStrLn "div/mod fold-runtime agreement ok"
