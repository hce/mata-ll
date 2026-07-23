module Main where
class Greet a where
  greet :: a -> String
instance Greet Int where
  greet _ = "first"
instance Greet Int where
  greet _ = "second"
main :: IO ()
main = putStrLn (greet (5 :: Int))
