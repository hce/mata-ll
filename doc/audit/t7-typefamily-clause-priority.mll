module Main where
data Nat = Z | S Nat
type family F n where
  F 'Z = Int
  F n  = String
val :: F 'Z
val = 5
main :: IO ()
main = print val
