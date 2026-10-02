module ScopeHostLib (twice, scaled) where

export twice :: Int -> Int
twice x = x * 2

replicate :: Int -> Int -> Int
replicate n x = n * x

export scaled :: Int -> Int
scaled x = replicate 3 x
