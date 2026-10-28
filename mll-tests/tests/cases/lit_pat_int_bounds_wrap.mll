-- lua-compat-skip: luajit
-- The companion of lit_pat_int_bounds for values that need the host's
-- 64-bit integer subtype: `minBound + 1` is not a double (its nearest
-- double is -2^63, so a doubles-only host matches it against the
-- `minBound` pattern instead), and `negate minBound` wraps to itself only
-- with 64-bit integer arithmetic — the documented LuaJIT Int range limit,
-- not a spelling error.
module Main where

f :: Int -> String
f (-9223372036854775808) = "min"
f (-9223372036854775807) = "min-plus-one"
f 9223372036854775807 = "max"
f _ = "other"

h :: Integer -> String
h 9223372036854775808 = "past-max"
h _ = "other"

lo :: Int
lo = -9223372036854775807 - 1

main :: IO ()
main = do
    putStrLn (f (minBound + 1))
    putStrLn (f (maxBound - 1))
    putStrLn (f (lo + 1))
    putStrLn (h (toInteger (maxBound :: Int) + 1))
    print (negate lo == lo)
    print (lo - 1 == maxBound)
    print (lo * 2)
