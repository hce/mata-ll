module Main where
class Greet a where
  greet :: a -> String
instance Greet Integer where
  greet _ = "first"
instance Greet Integer where
  greet _ = "second"
main :: IO ()
main = putStrLn (greet (5 :: Integer))
