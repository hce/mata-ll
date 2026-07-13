-- Probe (Finding 3 adjunct — APPLIES TO ALL TARGETS), FLAGGED:
-- constant folding must agree with Haskell's FLOOR division on
-- negative divisors. mllc/src/fold.rs folds literal div/mod with
-- i64::div_euclid / rem_euclid — EUCLIDEAN division. For a positive
-- divisor euclidean == floor, so the folded positive cases elsewhere
-- are safe; for a NEGATIVE divisor they differ:
--
--     GHC:      7 `div` (-2) == -4      7 `mod` (-2) == -1
--     euclid:   7 div_euclid -2 == -3   7 rem_euclid -2 ==  1
--     GHC:    (-7) `div` (-2) ==  3   (-7) `mod` (-2) == -1
--     euclid: (-7) div_euclid -2 ==  4 (-7) rem_euclid -2 ==  1
--
-- If the negation of a literal reaches fold.rs as a literal, the
-- literal expressions below fold to the WRONG (euclidean) answers and
-- this file fails — that is a real, separate sub-bug to report, not a
-- test to weaken. If negated literals never fold, these run through
-- the same runtime path as div_mod_small_exact.mll and pass. Either
-- outcome is informative; expected results below are Haskell's.

main :: IO ()
main = do
    assert (((-7) :: Integer) `div` 2 == (-4))     "literal -7 div 2"
    assert (((-7) :: Integer) `mod` 2 == 1)        "literal -7 mod 2"
    assert ((7 :: Integer) `div` (-2) == (-4))     "literal 7 div -2 (floor, not euclid)"
    assert ((7 :: Integer) `mod` (-2) == (-1))     "literal 7 mod -2 (divisor sign)"
    assert (((-7) :: Integer) `div` (-2) == 3)     "literal -7 div -2 (floor, not euclid)"
    assert (((-7) :: Integer) `mod` (-2) == (-1))  "literal -7 mod -2 (divisor sign)"

    -- Folded and runtime routes must agree with each other, too.
    let a = 7 :: Integer
    let b = (-2) :: Integer
    assert (a `div` b == (7 :: Integer) `div` (-2)) "runtime route agrees with folded route (div)"
    assert (a `mod` b == (7 :: Integer) `mod` (-2)) "runtime route agrees with folded route (mod)"

    putStrLn "ok"
