module Main where
xs :: [Int]
xs = map (+ 1) (0 : xs)
main :: IO ()
main = print (take 4 xs)
