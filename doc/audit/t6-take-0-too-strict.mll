module Main where
main :: IO ()
main = print (take 0 (error "forced") :: [Integer])
