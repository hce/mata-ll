-- GHC cgrun026: Recursive data structures
-- Tests linked list and binary search tree operations

data List a = Nil | Cons a (List a)
    deriving (Show, Eq)

toList :: [a] -> List a
toList []     = Nil
toList (x:xs) = Cons x (toList xs)

fromList :: List a -> [a]
fromList Nil         = []
fromList (Cons x xs) = x : fromList xs

appendL :: List a -> List a -> List a
appendL Nil ys         = ys
appendL (Cons x xs) ys = Cons x (appendL xs ys)

lengthL :: List a -> Integer
lengthL Nil         = 0
lengthL (Cons _ xs) = 1 + lengthL xs

reverseL :: List Integer -> List Integer
reverseL xs = go Nil xs
  where
    go acc Nil         = acc
    go acc (Cons x rest) = go (Cons x acc) rest

data BST = BLeaf | BNode BST Integer BST

insert :: Integer -> BST -> BST
insert x BLeaf = BNode BLeaf x BLeaf
insert x (BNode l v r)
    | x < v    = BNode (insert x l) v r
    | x > v    = BNode l v (insert x r)
    | otherwise = BNode l v r

inorder :: BST -> [Integer]
inorder BLeaf = []
inorder (BNode l v r) = appendList (inorder l) (v : inorder r)

appendList :: [a] -> [a] -> [a]
appendList [] ys = ys
appendList (x:xs) ys = x : appendList xs ys

main :: IO ()
main = do
    let l1 = toList [1, 2, 3]
    let l2 = toList [4, 5]
    assert (lengthL l1 == 3) "lengthL"
    assert (fromList l1 == [1, 2, 3]) "fromList"
    assert (fromList (appendL l1 l2) == [1, 2, 3, 4, 5]) "appendL"
    assert (fromList (reverseL l1) == [3, 2, 1]) "reverseL"

    -- BST
    let tree = insert 5 (insert 3 (insert 7 (insert 1 (insert 9 BLeaf))))
    assert (inorder tree == [1, 3, 5, 7, 9]) "bst inorder"

    -- Insert duplicate
    let tree2 = insert 5 tree
    assert (inorder tree2 == [1, 3, 5, 7, 9]) "bst dup"

    putStrLn "ok"
