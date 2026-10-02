module ScopeStack where

data Stack = Stack [Int]

empty :: Stack
empty = Stack []

push :: Int -> Stack -> Stack
push x (Stack xs) = Stack (x : xs)

size :: Stack -> Int
size (Stack xs) = length xs
