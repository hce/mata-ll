-- MLL bindings for Lua 5.4 bitwise operations

xor :: Int -> Int -> LuaPure "__mll_bxor" Int
band :: Int -> Int -> LuaPure "__mll_band" Int
bor :: Int -> Int -> LuaPure "__mll_bor" Int
bnot :: Int -> LuaPure "__mll_bnot" Int
shiftL :: Int -> Int -> LuaPure "__mll_shl" Int
shiftR :: Int -> Int -> LuaPure "__mll_shr" Int
