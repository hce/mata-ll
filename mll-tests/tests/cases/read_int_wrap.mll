-- lua-compat-skip: luajit
-- `read` at Int narrows the exact integer the digits denote like GHC's
-- fromInteger: modulo 2^64 into the signed range, so a numeral past
-- maxBound wraps instead of leaking a float into an Int (N7:
-- `read "9223372036854775808" :: Int` was a Lua float, `n + 1` printed
-- 9223372036854775808 where GHC prints -9223372036854775807). Decimal,
-- hex, octal and binary numerals at and beyond both bounds, and the
-- maxBound/minBound round trips. LuaJIT skip: its Int is a double, so
-- these values are the nearest doubles there (the documented 2^53 window).
module Main where

tryRead :: String -> (String -> String) -> IO ()
tryRead s f = catch (seq (f s == f s) (putStrLn (show s <> " -> " <> f s))) (\_ -> putStrLn (show s <> " -> no parse"))

asInt :: String -> String
asInt s = show (read s :: Int)

main :: IO ()
main = do
    let n = read "9223372036854775808" :: Int
    print (n + 1)
    print (n `div` 2)
    print (n == minBound)
    mapM_ (\s -> tryRead s asInt)
        [ "9223372036854775807", "9223372036854775808", "-9223372036854775808", "-9223372036854775809"
        , "-   9223372036854775808", "18446744073709551616", "18446744073709551617", "12345678901234567890123"
        , "99999999999999999999999999999999999999999", "0x7FFFFFFFFFFFFFFF", "0x8000000000000000"
        , "0xFFFFFFFFFFFFFFFF", "0x10000000000000000", "0x10000000000000001", "-0x8000000000000000"
        , "0o777777777777777777777", "0o1777777777777777777777", "0o2000000000000000000000"
        , "0b1111111111111111111111111111111111111111111111111111111111111111"
        , "0b10000000000000000000000000000000000000000000000000000000000000000"
        , "(-9223372036854775808)", "( 9223372036854775808 )"
        ]
    print (read (show (maxBound :: Int)) == (maxBound :: Int), read (show (minBound :: Int)) == (minBound :: Int))
    print (read "9223372036854775807" + (1 :: Int), read "-9223372036854775808" - (1 :: Int))
