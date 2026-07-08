-- MLL bindings for Lua 5.4 string primitives

strByte :: String -> Integer -> LuaPure "string.byte" Integer
strLen :: String -> LuaPure "string.len" Integer
strSub :: String -> Integer -> Integer -> LuaPure "string.sub" String
strChar :: Integer -> LuaPure "string.char" String

-- Unpack a String into the integer code of each character, in order.
-- mata-ll's String is opaque (not [Char]), so this is the bridge that lets
-- you process a string's characters with ordinary list functions.
strToInts :: String -> [Integer]
strToInts s = go 1
  where
    n = strLen s
    go i = if i > n then [] else strByte s i : go (i + 1)
