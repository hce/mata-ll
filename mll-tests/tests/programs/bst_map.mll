-- A persistent binary search tree used as an ordered map, with Functor
-- and Foldable instances, a precedence-aware Show, deletion by in-order
-- successor, and structural queries (height, balance, min/max, range).
-- Leans on: user Functor/Foldable instances, showsPrec/showParen, the
-- Foldable-generic Prelude functions (sum, length, elem, maximum,
-- foldr), Data.Foldable.toList, Maybe results, polymorphic recursion
-- over an Ord-constrained key.
import LString
import Data.Foldable (toList)

data Tree k v = Leaf | Node (Tree k v) k v (Tree k v)

instance (Show k, Show v) => Show (Tree k v) where
    showsPrec _ Leaf = showString "Leaf"
    showsPrec d (Node l k v r) = showParen (d > 10)
        (showString "Node " . showsPrec 11 l . showString " " . showsPrec 11 k
            . showString " " . showsPrec 11 v . showString " " . showsPrec 11 r)

instance Functor (Tree k) where
    fmap _ Leaf = Leaf
    fmap f (Node l k v r) = Node (fmap f l) k (f v) (fmap f r)

instance Foldable (Tree k) where
    foldr _ z Leaf = z
    foldr f z (Node l _ v r) = foldr f (f v (foldr f z r)) l
    foldl _ z Leaf = z
    foldl f z (Node l _ v r) = foldl f (f (foldl f z l) v) r
    foldl' _ z Leaf = z
    foldl' f z (Node l _ v r) = let z' = foldl' f z l in z' `seq` foldl' f (f z' v) r

insert :: Ord k => k -> v -> Tree k v -> Tree k v
insert k v Leaf = Node Leaf k v Leaf
insert k v (Node l nk nv r) = case compare k nk of
    LT -> Node (insert k v l) nk nv r
    GT -> Node l nk nv (insert k v r)
    EQ -> Node l k v r

lookup' :: Ord k => k -> Tree k v -> Maybe v
lookup' _ Leaf = Nothing
lookup' k (Node l nk nv r) = case compare k nk of
    LT -> lookup' k l
    GT -> lookup' k r
    EQ -> Just nv

minKey :: Tree k v -> Maybe (k, v)
minKey Leaf = Nothing
minKey (Node Leaf k v _) = Just (k, v)
minKey (Node l _ _ _) = minKey l

maxKey :: Tree k v -> Maybe (k, v)
maxKey Leaf = Nothing
maxKey (Node _ k v Leaf) = Just (k, v)
maxKey (Node _ _ _ r) = maxKey r

delete :: Ord k => k -> Tree k v -> Tree k v
delete _ Leaf = Leaf
delete k (Node l nk nv r) = case compare k nk of
    LT -> Node (delete k l) nk nv r
    GT -> Node l nk nv (delete k r)
    EQ -> case (l, r) of
        (Leaf, _) -> r
        (_, Leaf) -> l
        _ -> case minKey r of
            Nothing -> l
            Just (sk, sv) -> Node l sk sv (delete sk r)

height :: Tree k v -> Int
height Leaf = 0
height (Node l _ _ r) = 1 + max (height l) (height r)

balanced :: Tree k v -> Bool
balanced Leaf = True
balanced (Node l _ _ r) = abs (height l - height r) <= 1 && balanced l && balanced r

keys :: Tree k v -> [k]
keys Leaf = []
keys (Node l k _ r) = keys l ++ [k] ++ keys r

assocs :: Tree k v -> [(k, v)]
assocs Leaf = []
assocs (Node l k v r) = assocs l ++ [(k, v)] ++ assocs r

fromList :: Ord k => [(k, v)] -> Tree k v
fromList = foldl (\t (k, v) -> insert k v t) Leaf

range :: Ord k => k -> k -> Tree k v -> [(k, v)]
range lo hi t = [(k, v) | (k, v) <- assocs t, k >= lo, k <= hi]

-- Rebuild a balanced tree from sorted pairs.
rebalance :: Ord k => Tree k v -> Tree k v
rebalance t = build (assocs t)
  where
    build [] = Leaf
    build ps =
        let mid = length ps `div` 2
            (k, v) = ps !! mid
        in Node (build (take mid ps)) k v (build (drop (mid + 1) ps))

main :: IO ()
main = do
    let t = fromList [(50, "fifty"), (30, "thirty"), (70, "seventy"), (20, "twenty"), (40, "forty"), (60, "sixty"), (80, "eighty"), (35, "thirty-five")]
    print t
    print (keys t)
    print (lookup' 40 t, lookup' 45 t)
    print (minKey t, maxKey t)
    print (height t, balanced t, length t)
    print (toList t)
    print (fmap strLen t)
    print (sum (fmap strLen t), maximum (fmap strLen t), elem "sixty" t, elem "six" t)
    print (foldr (\v acc -> v <> "," <> acc) "" t)
    print (foldl (\acc v -> acc <> v <> ";") "" t)
    let t2 = delete 30 (delete 80 (delete 50 t))
    print t2
    print (keys t2, height t2)
    let chain = fromList [(i, i * i) | i <- [1 .. 12 :: Int]]
    print (height chain, balanced chain, height (rebalance chain), balanced (rebalance chain))
    print (range 4 8 chain)
    print (rebalance chain)
    print (sum chain, product (fmap (\x -> x `mod` 7 + 1) chain))
    print (keys (delete 99 chain) == keys chain)
    print (fmap (\s -> (s, s <> "!")) (fromList [(1 :: Int, "a"), (0, "b")]))
