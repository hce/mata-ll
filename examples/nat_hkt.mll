-- Higher-kinded types in mata-ll.
--
-- `data Nat = Zero | Succ Nat` is an ordinary type of kind `Type` — the Peano
-- naturals. The *higher-kinded* part is `Tree`, a type CONSTRUCTOR of kind
-- `Type -> Type`. `Functor` and `Foldable` abstract over exactly such an
-- `f :: Type -> Type`, so one `fmap` and one `foldr` work for a `Tree` of any
-- element type — Nat included.
--
-- The program builds a search tree of Peano numbers, then:
--   * reads the elements back in order and counts them  (Foldable)
--   * maps `Succ` over every element                     (Functor)
--   * folds the whole tree down to a single Nat          (Foldable)
--
-- Oracles (no external check needed): the printed lines are fixed. Elements
-- come back sorted 1..9; after `fmap Succ` they are 2..10; the node count is 9;
-- and the summed Nat is 45 + 9 = 54.

import Data.Foldable (toList)

-- The Peano naturals (`data Nat = Zero | Succ Nat`, kind Type) live in their
-- own module; we use them here purely as the element type of a higher-kinded
-- container.
import Nat (Nat(..), addNat, toInt, fromInt)

-- A binary tree: kind Type -> Type. THIS is the higher-kinded constructor the
-- two instances below abstract over.
data Tree a = Leaf | Node (Tree a) a (Tree a)

-- Functor: a single `fmap` transforms the elements of any `Tree a`.
instance Functor Tree where
    fmap _ Leaf         = Leaf
    fmap f (Node l x r) = Node (fmap f l) (f x) (fmap f r)

-- Foldable: providing `foldr` gives us toList/length/sum-by-fold for free.
-- Recursing right-subtree-first yields the elements in-order (left to right).
instance Foldable Tree where
    foldr _ z Leaf         = z
    foldr f z (Node l x r) = foldr f (f x (foldr f z r)) l
    foldl _ z Leaf         = z
    foldl f z (Node l x r) = foldl f (f (foldl f z l) x) r

-- Insert an Integer (stored as a Nat) into a BST keyed by its Integer value.
insert :: Integer -> Tree Nat -> Tree Nat
insert k Leaf = Node Leaf (fromInt k) Leaf
insert k (Node l x r)
  | k < toInt x = Node (insert k l) x r
  | k > toInt x = Node l x (insert k r)
  | otherwise   = Node l x r

fromList :: [Integer] -> Tree Nat
fromList = foldr insert Leaf

main :: IO ()
main = do
    let t = fromList [5, 3, 8, 1, 4, 7, 9, 2, 6]
    -- Foldable: in-order elements and the count, straight from the instance.
    putStrLn ("elements (in order): " <> show (map toInt (toList t)))
    putStrLn ("node count:          " <> show (length t))
    -- Functor: bump every Nat by one, entirely through fmap.
    let t' = fmap Succ t
    putStrLn ("after fmap Succ:     " <> show (map toInt (toList t')))
    -- Foldable again: collapse the whole tree into ONE Nat via addNat.
    let total = foldr addNat Zero t'
    putStrLn ("sum as Nat -> Int:   " <> show (toInt total))
