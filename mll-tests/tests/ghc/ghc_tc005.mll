-- GHC tc005: Polymorphic recursion
-- Function instantiated at different types in recursive call

data Tree a = Leaf | Node (Tree a) a (Tree a)
    deriving (Show, Eq)

insertT :: (a -> a -> Bool) -> a -> Tree a -> Tree a
insertT _  x Leaf = Node Leaf x Leaf
insertT lt x (Node l v r)
    | lt x v    = Node (insertT lt x l) v r
    | otherwise = Node l v (insertT lt x r)

appendList :: [a] -> [a] -> [a]
appendList [] ys     = ys
appendList (x:xs) ys = x : appendList xs ys

toList :: Tree a -> [a]
toList Leaf         = []
toList (Node l v r) = appendList (toList l) (v : toList r)

-- Build an Integer tree
buildIntTree :: [Integer] -> Tree Integer
buildIntTree [] = Leaf
buildIntTree (x:xs) = insertT (<) x (buildIntTree xs)

-- Build a String tree using same polymorphic insertT
buildStrTree :: [String] -> Tree String
buildStrTree [] = Leaf
buildStrTree (x:xs) = insertT (<) x (buildStrTree xs)

depthT :: Tree a -> Integer
depthT Leaf         = 0
depthT (Node l _ r) = 1 + max (depthT l) (depthT r)

main :: IO ()
main = do
    let it = buildIntTree [3, 1, 4, 1, 5, 9, 2, 6]
    let il = toList it
    assert (il == [1, 1, 2, 3, 4, 5, 6, 9]) "int tree sorted"
    assert (depthT it > 0) "int tree depth"

    let st = buildStrTree ["banana", "apple", "cherry"]
    let sl = toList st
    assert (sl == ["apple", "banana", "cherry"]) "string tree sorted"

    -- Same polymorphic function at different numeric type
    let it2 = buildIntTree [10, 5, 15, 3, 7]
    assert (toList it2 == [3, 5, 7, 10, 15]) "int tree 2"

    putStrLn "ok"
