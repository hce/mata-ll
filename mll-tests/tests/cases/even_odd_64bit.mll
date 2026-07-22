-- lua-compat-skip: luajit
-- Parity beyond the double mantissa: 2^53 is even, 2^53 + 1 is odd.
-- Needs 64-bit integers (Lua 5.4/5.5). LuaJIT numbers are doubles, so
-- 9007199254740993 is not representable there — the literal itself rounds
-- to the even neighbor — hence the compat skip, same convention as the
-- LBit/ByteString 64-bit cases.

main :: IO ()
main = do
    assert (even 9007199254740992)      "even 2^53"
    assert (odd 9007199254740993)       "odd 2^53+1"
    assert (odd (-9007199254740993))    "odd -(2^53+1)"
    putStrLn "even_odd_64bit ok"
