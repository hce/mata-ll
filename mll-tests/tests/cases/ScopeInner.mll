module ScopeInner where

step :: Int -> Int
step x = x + 1

twice :: Int -> Int
twice x = step (step x)

helper :: Int -> Int
helper x = x + 1000

other :: Int -> Int
other x = helper x
