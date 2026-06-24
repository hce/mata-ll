-- Small string helpers the rest of the interpreter leans on. String is an
-- opaque type here (not [Char]), so anything character-level goes through the
-- string.* primitives in LString.
module Util (concatStr, upperStr, joinStr) where

import LString (strChar, strToInts)

-- Concatenate a list of strings. (`concat` is for lists; String is opaque.)
concatStr :: [String] -> String
concatStr = foldr (<>) ""

-- Join a list of strings with a separator between them.
joinStr :: String -> [String] -> String
joinStr _   []       = ""
joinStr _   [x]      = x
joinStr sep (x:rest) = x <> sep <> joinStr sep rest

-- Upper-case the ASCII letters in a string (BASIC is case-insensitive).
upperStr :: String -> String
upperStr s = concatStr (map (\b -> strChar (toUpperByte b)) (strToInts s))

toUpperByte :: Integer -> Integer
toUpperByte b = if b >= 97 && b <= 122 then b - 32 else b
