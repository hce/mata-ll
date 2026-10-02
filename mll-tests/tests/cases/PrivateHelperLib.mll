module PrivateHelperLib (scaled, shifted) where

helper :: Int -> Int
helper x = x + 1

step :: Int
step = 10

scaled :: Int -> Int
scaled x = helper x * 2

shifted :: Int -> Int
shifted x = x + step
