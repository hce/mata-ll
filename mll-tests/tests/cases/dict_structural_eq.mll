module Main where

data W = W Int

instance Eq W where
  (==) (W a) (W b) = (a `mod` 2) == (b `mod` 2)

deepL :: Eq a => Int -> a -> a -> Bool
deepL 0 x y = x /= y
deepL n x y = deepL (n - 1) [x] [y]

deepM :: Eq a => Int -> a -> a -> Bool
deepM 0 x y = x == y
deepM n x y = deepM (n - 1) (Just x) (Just y)

deepT :: Eq a => Int -> a -> a -> Bool
deepT 0 x y = x == y
deepT n x y = deepT (n - 1) (x, x) (y, y)

main :: IO ()
main = do
  print (deepL 3 (W 2) (W 4))
  print (deepL 3 (W 1) (W 4))
  print (deepM 3 (W 2) (W 4))
  print (deepM 3 (W 1) (W 2))
  print (deepT 2 (W 2) (W 4))
  print (deepT 2 (W 1) (W 4))

-- expect: False
-- expect: True
-- expect: True
-- expect: False
-- expect: True
-- expect: False
