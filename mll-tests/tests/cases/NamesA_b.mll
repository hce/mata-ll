module NamesA_b where

data Tok = Tok2 Int deriving (Show, Eq, Ord)

step :: Int -> Int
step n = if n > 100 then n + 1 else step (n * 3)

show_Int :: Int -> String
show_Int n = "NamesA_b:" <> show n

eq_Int :: Int -> Int -> Bool
eq_Int a b = a + 1 == b
