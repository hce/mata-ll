module ScopeRight where

helper :: Int -> Int
helper x = x * 100

pick :: String -> String
pick s = s <> "!"

rightG :: Int -> Int
rightG x = helper x
