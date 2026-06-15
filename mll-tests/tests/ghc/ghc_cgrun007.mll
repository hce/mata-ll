-- GHC cgrun007: Tree ADT with height calculation

data Tree a = Leaf a | Branch (Tree a) (Tree a)

height :: Tree a -> Integer
height (Leaf _) = 1
height (Branch t1 t2) = 1 + max (height t1) (height t2)

main :: IO ()
main = putStrLn (show (height our_tree))
  where
    our_tree =
      Branch (Branch (Leaf 1) (Branch (Branch (Leaf 1) (Leaf 1)) (Leaf 1)))
             (Branch (Leaf 1) (Leaf 1))
