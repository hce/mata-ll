module Main where
type family Grow x where
  Grow x = Grow (Maybe x)
f :: Grow Int -> Int
f _ = 0
main :: IO ()
main = putStrLn "x"
