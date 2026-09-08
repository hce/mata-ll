{-# LANGUAGE GHC2021 #-}

-- LString: the GHC twin of lib/LString.mll for the program corpus
-- (tests/programs/, see regenerate-ghc-goldens.sh). mata-ll's String is
-- an opaque Lua string and LString is its character-level bridge; under
-- GHC the same names operate on [Char] with Lua's string-library
-- semantics, so a program written against LString runs unchanged under
-- both. Only the functions lib/LString.mll exports are provided, with
-- the exact signatures the mata-ll module declares.
--
-- Lua index rules ported here: positions are 1-based; a negative index
-- counts from the end (-1 is the last byte); strSub clamps its bounds to
-- the string (i < 1 becomes 1, j > len becomes len) and yields "" when
-- the clamped range is empty. strByte on a position outside the string is
-- a GHC error — under Lua the primitive returns nothing, which the
-- mata-ll runtime turns into a Lua error at the first arithmetic use — so
-- the corpus never indexes outside the string on purpose (the twin would
-- abort during golden generation, which is the loud failure we want).
--
-- Character codes are Unicode code points under GHC and bytes under
-- Lua; the corpus stays within ASCII so the two coincide.
module LString
  ( strByte, strLen, strSub, strChar, strToInts
  ) where

import Data.Char (chr, ord)

-- Resolve a Lua string index (1-based, negatives from the end) to a
-- 1-based position; the result may still lie outside 1..len.
luaPos :: Int -> Int -> Int
luaPos len i
  | i < 0     = len + i + 1
  | otherwise = i

strByte :: String -> Int -> Int
strByte s i
  | p >= 1 && p <= len = ord (s !! (p - 1))
  | otherwise = error ("strByte: index " ++ show i ++ " outside a string of length " ++ show len)
  where
    len = length s
    p = luaPos len i

strLen :: String -> Int
strLen = length

strSub :: String -> Int -> Int -> String
strSub s i j
  | from > to = ""
  | otherwise = take (to - from + 1) (drop (from - 1) s)
  where
    len = length s
    from = max 1 (luaPos len i)
    to = min len (luaPos len j)

strChar :: Int -> String
strChar c = [chr c]

strToInts :: String -> [Int]
strToInts = map ord
