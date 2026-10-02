module ScopeLeft where

helper :: Int -> Int
helper x = x + 1

pick :: Int -> Int
pick x = x + 7

leftF :: Int -> Int
leftF x = helper x
