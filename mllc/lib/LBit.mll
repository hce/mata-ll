-- MLL bindings for Lua 5.4 bitwise operations

xor :: Integer -> Integer -> LuaPure "__mll_bxor" Integer
band :: Integer -> Integer -> LuaPure "__mll_band" Integer
bor :: Integer -> Integer -> LuaPure "__mll_bor" Integer
bnot :: Integer -> LuaPure "__mll_bnot" Integer
shiftL :: Integer -> Integer -> LuaPure "__mll_shl" Integer
shiftR :: Integer -> Integer -> LuaPure "__mll_shr" Integer
