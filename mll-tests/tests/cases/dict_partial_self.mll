module Main where

data W = W Int

instance Eq W where
  (==) (W a) (W b) = (a `mod` 2) == (b `mod` 2)

deepP :: Eq a => Int -> a -> a -> Bool
deepP 0 x y = x /= y
deepP n x y = head (map (deepP (n - 1) [x]) [[y]])

main :: IO ()
main = do
  print (deepP 3 (W 2) (W 4))
  print (deepP 3 (W 1) (W 4))

-- expect: False
-- expect: True
