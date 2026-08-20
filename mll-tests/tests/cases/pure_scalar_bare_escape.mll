-- A `pure e` escaping to a caller's __mll_run is left BARE (no __mll_pure
-- box) exactly when e is provably WHNF and its type's runtime value can
-- never be a Lua function (ty_never_lua_function). Integer (the boxed
-- bignum table) and ByteString (a Lua string) joined the scalar list;
-- these bind-and-use round trips pin that the bare escape is
-- indistinguishable from the boxed one.

passBack :: ByteString -> IO ByteString
passBack b = do
    let n = bsLength b
    if n > 0 then pure b else pure b

passBig :: Integer -> IO Integer
passBig i = do
    let d = i + 1
    if d > 0 then return i else return i

main :: IO ()
main = do
    b <- passBack (bsReplicate 3 65)
    assert (bsLength b == 3) "bytestring escapes bare and binds"
    assert (bsToString b == "AAA") "bytestring value intact"
    i <- passBig (2 ^ 70)
    assert (i == 1180591620717411303424) "integer escapes bare and binds"
    assert (i * 2 == 2361183241434822606848) "integer arithmetic after bind"
    putStrLn "pure scalar bare escape ok"
-- expect: pure scalar bare escape ok
