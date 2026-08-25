-- lua-compat-skip: luajit
-- GHC's Bounded deriving rule: an enumeration (first/last constructor)
-- OR a single-constructor product whose fields are all Bounded — the
-- product path was rejected ("must be a simple enum").  Bounded Int and
-- Bounded Bool are GHC-parity builtins now (LuaJIT skip: Int bounds are
-- doubles there, the usual >2^53 degradation).

data Pair = MkPair Int Bool deriving (Show, Bounded)

data Flag = Off | On deriving (Show, Eq, Bounded)

main :: IO ()
main = do
    print (minBound :: Pair)
    print (maxBound :: Pair)
    print (minBound :: Int)
    print (maxBound :: Int)
    print (minBound :: Bool)
    print (maxBound :: Bool)
    print (minBound :: Flag)
    print (maxBound :: Flag)
