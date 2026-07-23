-- GHC cgrun042: Complex pattern matching
-- Tests nested constructors, wildcards, multiple clauses

data Tree a = Leaf | Node (Tree a) a (Tree a)
    deriving (Show, Eq)

-- Count nodes
size :: Tree a -> Int
size Leaf = 0
size (Node l _ r) = 1 + size l + size r

-- Depth
depth :: Tree a -> Int
depth Leaf = 0
depth (Node l _ r) = 1 + max (depth l) (depth r)

-- Mirror a tree
mirror :: Tree a -> Tree a
mirror Leaf = Leaf
mirror (Node l v r) = Node (mirror r) v (mirror l)

-- Insert into BST
bstInsert :: Int -> Tree Int -> Tree Int
bstInsert x Leaf = Node Leaf x Leaf
bstInsert x (Node l v r)
    | x < v    = Node (bstInsert x l) v r
    | x > v    = Node l v (bstInsert x r)
    | otherwise = Node l v r

-- In-order traversal
inorder :: Tree Int -> [Int]
inorder Leaf = []
inorder (Node l v r) = appendList (inorder l) (v : inorder r)

appendList :: [a] -> [a] -> [a]
appendList [] ys = ys
appendList (x:xs) ys = x : appendList xs ys

-- Build from list
fromList :: [Int] -> Tree Int
fromList [] = Leaf
fromList (x:xs) = bstInsert x (fromList xs)

main :: IO ()
main = do
    let t = Node (Node Leaf 1 Leaf) 2 (Node Leaf 3 Leaf)
    assert (size t == 3) "size 3"
    assert (depth t == 2) "depth 2"
    assert (size Leaf == 0) "size leaf"

    -- Mirror swaps children
    let m = mirror t
    assert (m == Node (Node Leaf 3 Leaf) 2 (Node Leaf 1 Leaf)) "mirror"
    assert (mirror (mirror t) == t) "mirror mirror"

    -- BST
    let bst = fromList [5, 3, 7, 1, 4, 6, 8]
    assert (inorder bst == [1, 3, 4, 5, 6, 7, 8]) "bst inorder"
    assert (size bst == 7) "bst size"

    putStrLn "ok"
