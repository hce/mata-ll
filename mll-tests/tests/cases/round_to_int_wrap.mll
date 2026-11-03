-- lua-compat-skip: luajit
-- `floor`, `ceiling`, `truncate` and `round` of a Number at or beyond
-- 2^63 are GHC's `fromInteger . floor`: the exact integer reduced modulo
-- 2^64 into Int's range (N12/Q35; the Prelude's raw math.floor/math.ceil
-- returned the float, so `floor 1e30 + 1` printed a 31-digit numeral and
-- `floor 1e300` a 301-digit one). This case does Int ARITHMETIC on the
-- wrapped results — `+ 1`, `- minBound`, `mod`, comparisons against
-- `maxBound - k` — which needs the exact 64-bit Int of Lua 5.3+; the
-- values themselves, and everything that holds on a doubles-only host,
-- are pinned in round_to_int_nonfinite. The original probe's lines are
-- kept in full at the end.
module Main where

dv :: Number -> Number -> Number
dv a b = a / b

fl :: Number -> Int
fl x = floor x

main :: IO ()
main = do
    print (fl 1.0e30 + 1)
    print (fl 1.0e30 == 5076964154930102272, fl 9223372036854775808.0 == minBound, fl 9223372036854774784.0 == maxBound - 1023)
    print ((ceiling (1.0e19 :: Number) :: Int) - 1, fl 9223372036854777856.0 - minBound, fl (-9223372036854777856.0) - maxBound)
    print ((truncate (9.3e18 :: Number) :: Int) + 9223372036854775807, fl 1.0e300 + fl 1.0e30 * 2)
    print (fl 1.0e30 `mod` 1000, fl 1.0e30 `div` 1000000000, negate (fl 1.0e30) == fl (-1.0e30))
    print (round (1.0e30 :: Number) + 1 :: Int, (ceiling (2.5e30 :: Number) :: Int) `rem` 10)
    print (fl (dv 0.0 0.0), fl (dv 1.0 0.0), fl (dv (-1.0) 0.0))
    print (ceiling (dv 0.0 0.0) :: Int, truncate (dv 1.0 0.0) :: Int)
    print (fl 1.0e300, ceiling (1.0e19 :: Number) :: Int)
    print (truncate (-1.0e19 :: Number) :: Int, truncate (9.3e18 :: Number) :: Int)
    print (fl 9007199254740993.0, fl (-2.5), ceiling (-2.5 :: Number) :: Int, truncate (-2.5 :: Number) :: Int)
    print (round (2.5 :: Number) :: Int, round (3.5 :: Number) :: Int, round (-2.5 :: Number) :: Int, round (1.0e30 :: Number) :: Int)
    print (round (dv 1.0 0.0) :: Int)
    print (round (dv 0.0 0.0) :: Int)
