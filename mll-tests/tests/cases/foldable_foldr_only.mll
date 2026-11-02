-- A Foldable instance that gives foldr alone — GHC's minimal complete
-- definition — is complete: foldl is the class default (the
-- continuation-passing fold over foldr base's Dual/Endo definition
-- amounts to), foldl' its strict twin, and every Prelude fold goes
-- through them. The lazy fold never looks at an element the step
-- function ignores.

import Data.Foldable (toList)

data Tree a = Leaf | Node (Tree a) a (Tree a)

instance Foldable Tree where
    foldr _ z Leaf = z
    foldr f z (Node l x r) = foldr f (f x (foldr f z r)) l

t :: Tree Int
t = Node (Node Leaf 1 Leaf) 2 (Node (Node Leaf 3 Leaf) 4 (Node Leaf 5 Leaf))

bombs :: Tree Int
bombs = Node (Node Leaf (error "left leaf forced") Leaf) (error "root forced") (Node Leaf 7 Leaf)

main :: IO ()
main = do
    print (foldr (:) [] t)
    print (foldl (flip (:)) [] t)
    print (foldl (\acc x -> acc * 10 + x) 0 t)
    print (foldl (-) 100 t, foldr (-) 100 t)
    print (foldl' (+) 0 t, foldl (+) 0 (Leaf :: Tree Int))
    print (foldl (\_ x -> x) 0 bombs)
    print (sum t, product t, length t, null t, null (Leaf :: Tree Int))
    print (elem 3 t, elem 6 t, notElem 6 t)
    print (maximum t, minimum t, foldr1 max t, foldl1 min t)
    print (toList t, concatMap (\x -> [x, x]) t)
    print (and (fmapList even t), or (fmapList even t), any (> 4) t, all (> 0) t)
    mapM_ print t
  where
    fmapList p tr = map p (toList tr)
