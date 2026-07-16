module Main where
type family Grow x where
  Grow x = Grow (Maybe x)
f :: Grow Integer -> Integer
f _ = 0
main :: IO ()
main = putStrLn "x"
