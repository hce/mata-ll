-- A parameterized instance's NULLARY method through dictionary passing
-- (F23): the dictform must be a function over the context dictionaries
-- (it emitted as a CAF thunk with the dict parameter free), and the
-- constructed dictionary's field must hold the VALUE the way static
-- dictionaries do (a wrapper closure flowed where `[def]` was expected).
module Main where

class Def a where
  def :: a

instance Def Int where
  def = 7

instance Def a => Def [a] where
  def = [def]

defAt :: Def a => Int -> a
defAt 0 = def
defAt n = defAt (n - 1)

deep :: Def a => Int -> a -> a
deep 0 _ = defAt 0
deep n x = head (deep (n - 1) [x])

main :: IO ()
main = do
  print (deep 2 (5 :: Int))
  print (defAt 3 :: Int)

-- expect: 7
-- expect: 7
