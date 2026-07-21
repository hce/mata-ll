-- Turns a line of BASIC source into a flat list of tokens by scanning the
-- raw bytes (String is opaque, so we walk it with the string.* primitives).
module Lexer (tokenize) where

import LString (strByte, strLen, strSub)
import Tokens (Token(..))
import Util (upperStr)

tokenize :: String -> [Token]
tokenize s = go 1
  where
    n = strLen s

    -- Byte at a 1-based position; 0 stands for "past the end".
    at i = if i > n then 0 else strByte s i

    go i =
      if i > n then []
      else
        let c = at i
        in if c == 32 then go (i + 1)                 -- space
           else if c == 39 then []                    -- ' begins a comment
           else if isDigit c then number i i False
           else if c == 46 then number i i True       -- leading-dot number
           else if isAlpha c then word i i
           else if c == 34 then string (i + 1) (i + 1) -- opening quote
           else if c == 60 then lt i                  -- <  <=  <>
           else if c == 62 then gt i                  -- >  >=
           else TOp (strSub s i i) : go (i + 1)

    -- A numeric literal: digits with at most one decimal point.
    number start i seenDot =
      let c = at i
      in if isDigit c then number start (i + 1) seenDot
         else if c == 46 && not seenDot then number start (i + 1) True
         else TNum (read_Number (strSub s start (i - 1))) : go i

    -- An identifier or keyword: letters/digits, optional trailing $.
    word start i =
      let c = at i
      in if isAlpha c || isDigit c then word start (i + 1)
         else if c == 36 then TWord (upperStr (strSub s start i)) : go (i + 1)
         else TWord (upperStr (strSub s start (i - 1))) : go i

    -- A string literal: everything up to the closing quote (or end of line).
    string start i =
      if i > n then [TStr (strSub s start (i - 1))]
      else
        let c = at i
        in if c == 34 then TStr (strSub s start (i - 1)) : go (i + 1)
           else string start (i + 1)

    lt i =
      let c2 = at (i + 1)
      in if c2 == 61 then TOp "<=" : go (i + 2)
         else if c2 == 62 then TOp "<>" : go (i + 2)
         else TOp "<" : go (i + 1)

    gt i =
      let c2 = at (i + 1)
      in if c2 == 61 then TOp ">=" : go (i + 2)
         else TOp ">" : go (i + 1)

isDigit :: Integer -> Bool
isDigit c = c >= 48 && c <= 57

isAlpha :: Integer -> Bool
isAlpha c = (c >= 65 && c <= 90) || (c >= 97 && c <= 122)
