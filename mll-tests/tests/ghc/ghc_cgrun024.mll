-- GHC cgrun024: Case expressions with nested patterns
-- Tests case-of with various pattern shapes

data Token = TNum Integer | TOp String | TEnd
    deriving (Show, Eq)

classifyNum :: Integer -> String
classifyNum n
    | n < 0     = "negative"
    | n == 0    = "zero"
    | n < 100   = "small"
    | otherwise = "large"

classifyToken :: Token -> String
classifyToken (TNum n)  = "number: " <> classifyNum n
classifyToken (TOp s)   = "op: " <> s
classifyToken TEnd      = "end"

main :: IO ()
main = do
    assert (classifyToken (TNum (-5)) == "number: negative") "neg"
    assert (classifyToken (TNum 0) == "number: zero") "zero"
    assert (classifyToken (TNum 42) == "number: small") "small"
    assert (classifyToken (TNum 999) == "number: large") "large"
    assert (classifyToken (TOp "+") == "op: +") "plus"
    assert (classifyToken (TOp "-") == "op: -") "minus"
    assert (classifyToken TEnd == "end") "end"
    putStrLn "ok"
