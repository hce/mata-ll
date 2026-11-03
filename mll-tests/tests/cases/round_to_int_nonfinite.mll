-- `floor`, `ceiling`, `truncate` and `round` of a Number always yield an
-- Int, on every host (N12/Q35): the Prelude bound them to Lua's
-- math.floor/math.ceil, which return the FLOAT when no integer fits, so a
-- NaN, an infinity or a magnitude of 2^63 and beyond came out as an
-- `Int` holding `NaN`, `Infinity` or a 301-digit numeral, and `round` of
-- NaN or an infinity never returned (its `fromIntegral n` sent the
-- Integer limb loop around forever). GHC's `floor :: Double -> Int` is
-- `fromInteger . floor`: the exact integer modulo 2^64, and 0 for NaN and
-- the infinities. Every line here holds on Lua 5.3+ and on a doubles-only
-- host alike — a wrapped value is a multiple of 2^11, exact as a double,
-- so LuaJIT prints GHC's digits too; the half-to-even ties, the shapes
-- that produce a -0.0 Int on LuaJIT, the 2^53 neighbourhood, and the
-- conversions of the results. Arithmetic on the wrapped results past 2^53
-- is in round_to_int_wrap (Lua 5.3+).
module Main where

dv :: Number -> Number -> Number
dv a b = a / b

nan :: Number
nan = dv 0.0 0.0

inf :: Number
inf = dv 1.0 0.0

fl :: Number -> Int
fl x = floor x

main :: IO ()
main = do
    print (fl nan, fl inf, fl (negate inf))
    print (ceiling nan :: Int, ceiling inf :: Int, ceiling (negate inf) :: Int)
    print (truncate nan :: Int, truncate inf :: Int, truncate (negate inf) :: Int)
    print (round nan :: Int, round inf :: Int, round (negate inf) :: Int)
    print (fromIntegral (fl nan) :: Number, toInteger (round inf :: Int), even (fl inf), fl nan == 0)
    print (round 0.5 :: Int, round 1.5 :: Int, round 2.5 :: Int, round 3.5 :: Int)
    print (round (-0.5) :: Int, round (-1.5) :: Int, round (-2.5) :: Int, round (-3.5) :: Int)
    print (round 4503599627370496.5 :: Int, round 4503599627370497.5 :: Int)
    print (round (-0.49999999999999994) :: Int, round 0.49999999999999994 :: Int, round (-0.4) :: Int, round 0.6 :: Int)
    print (fl (-0.0), ceiling (-0.5 :: Number) :: Int, truncate (-0.5 :: Number) :: Int, round (-0.25 :: Number) :: Int)
    print (fl 9007199254740991.0, fl 9007199254740992.0, fl 9007199254740993.0, ceiling (9007199254740991.0 :: Number) :: Int)
    print (fl (-2.5), ceiling (-2.5 :: Number) :: Int, truncate (-2.5 :: Number) :: Int, truncate (2.5 :: Number) :: Int)
    print (fl 1.0e30, round (1.0e30 :: Number) :: Int, fl (-1.0e30), round (-1.0e30 :: Number) :: Int)
    print (ceiling (1.0e19 :: Number) :: Int, truncate (-1.0e19 :: Number) :: Int, truncate (9.3e18 :: Number) :: Int)
    print (fl 1.0e300, fl (-1.0e300), ceiling (2.5e30 :: Number) :: Int, truncate (2.5e30 :: Number) :: Int)
    print (fl 9223372036854775808.0, fl (-9223372036854775808.0), ceiling (9223372036854775807.0 :: Number) :: Int)
    print (fl 9223372036854774784.0, fl 9223372036854777856.0, fl (-9223372036854777856.0))
    print (toInteger (fl 1.0e30), fromIntegral (fl 1.0e30) :: Number, toInteger (ceiling (1.0e19 :: Number) :: Int))
