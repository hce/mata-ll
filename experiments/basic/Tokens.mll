-- Token type produced by the lexer and consumed by the parser.
module Tokens (Token(..), tokenText) where

-- TNum: numeric literal.  TStr: string-literal contents (no quotes).
-- TWord: identifier or keyword, upper-cased, a trailing $ kept.
-- TOp: operator or punctuation (+ - * / ^ = <> < > <= >= ( ) , ; :).
data Token = TNum Number | TStr String | TWord String | TOp String
  deriving (Show, Eq)

-- A human-readable rendering, handy for error messages.
tokenText :: Token -> String
tokenText (TNum _)  = "<number>"
tokenText (TStr _)  = "<string>"
tokenText (TWord w) = w
tokenText (TOp o)   = o
