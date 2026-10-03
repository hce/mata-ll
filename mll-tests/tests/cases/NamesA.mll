module NamesA where

data Tok = Tok Int deriving (Show, Eq, Ord)

b_step :: Int -> Int
b_step n = if n > 100 then n else b_step (n * 2)

show_Int :: Int -> String
show_Int n = "NamesA:" <> show n

show_Tok :: Tok -> String
show_Tok (Tok n) = "tok " <> show n

scale :: Int -> Int -> Int
scale map show_Int = map * 10 + show_Int

render :: Int -> String
render n = show n <> "/" <> show [n] <> "/" <> show (Tok n)
