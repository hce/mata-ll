module Main where

class Mk a where
  mk :: Int -> a

instance Mk Int where
  mk n = n

instance Mk a => Mk [a] where
  mk n = [mk n]

defAt :: Mk a => Int -> a
defAt 0 = mk 0
defAt n = head (defAt (n - 1))

main :: IO ()
main = print (defAt 3 :: Int)

-- expect: 0
