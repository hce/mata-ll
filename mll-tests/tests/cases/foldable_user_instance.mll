-- User-defined Foldable and Traversable instances for a custom type.
-- Every generic Prelude function (sum, length, null, maximum, foldMap,
-- sequenceA) and Data.Foldable's toList must work through them, and
-- traverse must run with both the Maybe and the IO applicative.

import Data.Foldable (toList)

data Tree a = Leaf | Node (Tree a) a (Tree a)
    deriving (Functor)

instance Foldable Tree where
    foldr _ z Leaf = z
    foldr f z (Node l x r) = foldr f (f x (foldr f z r)) l
    foldl _ z Leaf = z
    foldl f z (Node l x r) = foldl f (f (foldl f z l) x) r

instance Traversable Tree where
    traverse _ Leaf = pure Leaf
    traverse f (Node l x r) =
        liftA2 (\l2 p -> Node l2 (fst p) (snd p))
               (traverse f l)
               (liftA2 (\x2 r2 -> (x2, r2)) (f x) (traverse f r))

t1 :: Tree Integer
t1 = Node (Node Leaf 1 Leaf) 2 (Node Leaf 3 Leaf)

half :: Integer -> Maybe Integer
half n = if mod n 2 == 0 then Just (div n 2) else Nothing

main :: IO ()
main = do
    assert (toList t1 == [1, 2, 3]) "toList Tree (in-order)"
    assert (sum t1 == 6) "sum Tree"
    assert (length t1 == 3) "length Tree"
    assert (null (Leaf :: Tree Integer)) "null Leaf"
    assert (not (null t1)) "null Node"
    assert (elem 2 t1) "elem Tree"
    assert (maximum t1 == 3) "maximum Tree"
    assert (minimum t1 == 1) "minimum Tree"
    assert (foldMap show t1 == "123") "foldMap Tree"
    -- fmap comes from deriving (Functor), the Traversable superclass
    assert (toList (fmap (\x -> x * 2) t1) == [2, 4, 6]) "fmap Tree"
    case traverse (\x -> Just (x * 10)) t1 of
        Just t2 -> assert (toList t2 == [10, 20, 30]) "traverse Tree all Just"
        Nothing -> error "traverse Tree: unexpected Nothing"
    case traverse half t1 of
        Nothing -> assert True "traverse Tree blocks on odd"
        Just _ -> error "traverse Tree: expected Nothing"
    rs <- traverse (\x -> pure (x + 100)) t1
    assert (toList rs == [101, 102, 103]) "traverse Tree with IO"
