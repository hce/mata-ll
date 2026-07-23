module Main where
class Pretty a where
  pretty :: a -> String
instance Pretty [Int] where
  pretty xs = "int-list body"
main :: IO ()
main = putStrLn (pretty [True, False])
