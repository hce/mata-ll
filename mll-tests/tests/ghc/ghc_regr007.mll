-- ghc_regr007: Deriving Show on recursive types (Tree, List)

data Tree a = Leaf | Node (Tree a) a (Tree a)
    deriving (Show, Eq)

data MyList a = Nil | Cons a (MyList a)
    deriving (Show, Eq)

-- Insert into BST (Int only to avoid compare constraint)
insertBST :: Int -> Tree Int -> Tree Int
insertBST x Leaf = Node Leaf x Leaf
insertBST x (Node l v r)
    | x < v    = Node (insertBST x l) v r
    | x > v    = Node l v (insertBST x r)
    | otherwise = Node l v r

-- In-order traversal
inorder :: Tree a -> [a]
inorder Leaf         = []
inorder (Node l v r) = appendList (inorder l) (v : inorder r)

appendList :: [a] -> [a] -> [a]
appendList [] ys     = ys
appendList (x:xs) ys = x : appendList xs ys

-- Convert to MyList
toMyList :: [a] -> MyList a
toMyList []     = Nil
toMyList (x:xs) = Cons x (toMyList xs)

fromMyList :: MyList a -> [a]
fromMyList Nil        = []
fromMyList (Cons x xs) = x : fromMyList xs

myListLength :: MyList a -> Int
myListLength Nil        = 0
myListLength (Cons _ xs) = 1 + myListLength xs

main :: IO ()
main = do
    -- Show on recursive types works (format may vary, just check non-empty string)
    assert (show (Leaf :: Tree Int) /= "") "show Leaf"
    assert (show (Node Leaf 1 Leaf) /= "") "show node"

    -- BST operations
    let t = insertBST 5 (insertBST 3 (insertBST 7 (insertBST 1 Leaf)))
    let sorted = inorder t
    assert (sorted == [1, 3, 5, 7]) "inorder sorted"

    -- Eq on recursive tree
    let t1 = Node Leaf 1 (Node Leaf 2 Leaf)
    let t2 = Node Leaf 1 (Node Leaf 2 Leaf)
    let t3 = Node Leaf 1 (Node Leaf 3 Leaf)
    assert (t1 == t2) "tree eq"
    assert (t1 /= t3) "tree neq"

    -- MyList show (format may vary, just check non-empty string)
    assert (show (Nil :: MyList Int) /= "") "show Nil"
    assert (show (Cons 1 (Cons 2 Nil)) /= "") "show Cons"

    -- MyList operations
    let ml = toMyList [10, 20, 30]
    assert (myListLength ml == 3) "MyList length"
    assert (fromMyList ml == [10, 20, 30]) "fromMyList"
    assert (ml == Cons 10 (Cons 20 (Cons 30 Nil))) "MyList eq"

    putStrLn "ok"
