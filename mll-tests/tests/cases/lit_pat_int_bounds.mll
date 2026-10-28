-- Integer literal patterns at Int compare with the literal's exact 64-bit
-- value on every host: the only literal spelling of `minBound`,
-- `-9223372036854775808`, overflows i64 before its negation folds and
-- used to be compared as a bignum against a machine number (never true);
-- a literal beyond the i64 range at Int is GHC's wrapped `fromInteger`
-- (`9223372036854775808` is `minBound`, `18446744073709551617` is 1); and
-- the constant-folded `-9223372036854775807 - 1` is spelled so a
-- doubles-only host reads -2^63, not +2^63 (`n > lo` was False on
-- LuaJIT). Every value here is exact on a doubles-only host too (-2^63
-- and -2^62 are powers of two); the lines that need the 64-bit integer
-- subtype are in lit_pat_int_bounds_wrap.
module Main where

f :: Int -> String
f (-9223372036854775808) = "min"
f 9223372036854775807 = "max"
f _ = "other"

g :: Int -> String
g 9223372036854775808 = "wrapped-min"
g 18446744073709551617 = "wrapped-one"
g _ = "other"

h :: Integer -> String
h (-9223372036854775808) = "min"
h 9223372036854775808 = "past-max"
h _ = "other"

lo :: Int
lo = -9223372036854775807 - 1

caseMin :: Int -> String
caseMin n = case n of
    0 -> "zero"
    (-9223372036854775808) -> "min-arm"
    _ -> "other-arm"

main :: IO ()
main = do
    putStrLn (f minBound)
    putStrLn (f maxBound)
    putStrLn (f 0)
    putStrLn (g minBound)
    putStrLn (g 1)
    putStrLn (g 0)
    putStrLn (h (-9223372036854775808))
    putStrLn (h 9223372036854775808)
    putStrLn (h 5)
    putStrLn (caseMin (-9223372036854775807 - 1))
    putStrLn (caseMin lo)
    putStrLn (caseMin 0)
    print (lo < 0)
    n <- pure (5 :: Int)
    print (n > (-9223372036854775807 - 1))
    print (n > lo)
    print (lo + n < 0)
    print (lo == minBound)
    print lo
    print (lo `div` 2)
    print (lo `quot` 2)
    print (lo `div` (-2))
