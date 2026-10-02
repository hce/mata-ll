module FixityInner (inner, innerMixed) where

infixl 1 |>
(|>) :: Int -> Int -> Int
a |> b = a * b

infixr 5 `minus`
minus :: Int -> Int -> Int
minus a b = a - b

inner :: Int
inner = 2 |> 3 + 1

innerMixed :: Int
innerMixed = 10 `minus` 3 `minus` 2
