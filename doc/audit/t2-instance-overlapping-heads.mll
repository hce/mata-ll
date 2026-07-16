module Main where
data Pair a b = Pair a b
class Pretty a where
  pretty :: a -> String
instance Pretty (Pair Integer Integer) where
  pretty _ = "ints"
instance Pretty (Pair Bool Bool) where
  pretty _ = "bools"
i2 :: Pair Integer Integer
i2 = Pair 1 2
bb :: Pair Bool Bool
bb = Pair True False
main :: IO ()
main = do
  putStrLn (pretty i2)
  putStrLn (pretty bb)
