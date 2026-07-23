-- Stress test: a complete non-trivial program (balanced BST with insert/lookup/delete)

data BST = BSTLeaf | BSTNode Int BST BST
    deriving (Show, Eq)

bstInsert :: Int -> BST -> BST
bstInsert x BSTLeaf = BSTNode x BSTLeaf BSTLeaf
bstInsert x (BSTNode v l r)
    | x < v    = BSTNode v (bstInsert x l) r
    | x > v    = BSTNode v l (bstInsert x r)
    | otherwise = BSTNode v l r

bstMember :: Int -> BST -> Bool
bstMember _ BSTLeaf = False
bstMember x (BSTNode v l r)
    | x < v    = bstMember x l
    | x > v    = bstMember x r
    | otherwise = True

bstSize :: BST -> Int
bstSize BSTLeaf = 0
bstSize (BSTNode _ l r) = 1 + bstSize l + bstSize r

bstMin :: BST -> Int
bstMin BSTLeaf = error "empty tree"
bstMin (BSTNode v l _) = case l of
    BSTLeaf -> v
    _       -> bstMin l

bstMax :: BST -> Int
bstMax BSTLeaf = error "empty tree"
bstMax (BSTNode v _ r) = case r of
    BSTLeaf -> v
    _       -> bstMax r

bstDelete :: Int -> BST -> BST
bstDelete _ BSTLeaf = BSTLeaf
bstDelete x (BSTNode v l r)
    | x < v    = BSTNode v (bstDelete x l) r
    | x > v    = BSTNode v l (bstDelete x r)
    | otherwise = bstDeleteNode v l r

bstDeleteNode :: Int -> BST -> BST -> BST
bstDeleteNode _ BSTLeaf r = r
bstDeleteNode _ l BSTLeaf = l
bstDeleteNode _ l r =
    let successor = bstMin r
    in BSTNode successor l (bstDelete successor r)

bstToList :: BST -> [Int]
bstToList BSTLeaf = []
bstToList (BSTNode v l r) = appendList (bstToList l) (v : bstToList r)

appendList :: [a] -> [a] -> [a]
appendList [] ys = ys
appendList (x:xs) ys = x : appendList xs ys

isSorted :: [Int] -> Bool
isSorted [] = True
isSorted (_:[]) = True
isSorted (a:b:rest) = a <= b && isSorted (b : rest)

fromList :: [Int] -> BST
fromList [] = BSTLeaf
fromList (x:xs) = bstInsert x (fromList xs)

interleave :: [Int] -> [Int] -> [Int]
interleave [] ys = ys
interleave xs [] = xs
interleave (x:xs) (y:ys) = x : y : interleave xs ys

range :: Int -> Int -> [Int]
range a b = if a > b then [] else a : range (a + 1) b

main :: IO ()
main = do
    let odds = range 1 100
    let evens = range 101 200
    let mixed = interleave evens odds
    let t = fromList mixed
    assert (bstSize t == 200) "size 200"
    assert (isSorted (bstToList t)) "BST sorted"
    assert (bstMember 1 t) "member 1"
    assert (bstMember 100 t) "member 100"
    assert (bstMember 200 t) "member 200"
    assert (not (bstMember 0 t)) "not member 0"
    assert (not (bstMember 201 t)) "not member 201"
    assert (bstMin t == 1) "min is 1"
    assert (bstMax t == 200) "max is 200"
    let t2 = bstDelete 1 (bstDelete 100 (bstDelete 200 t))
    assert (bstSize t2 == 197) "size after deletes"
    assert (isSorted (bstToList t2)) "still sorted after deletes"
    assert (not (bstMember 1 t2)) "1 deleted"
    assert (not (bstMember 100 t2)) "100 deleted"
    assert (not (bstMember 200 t2)) "200 deleted"
    assert (bstMember 2 t2) "2 still present"
    assert (bstMember 199 t2) "199 still present"
    let allElems = bstToList t
    let t3 = foldl (\acc x -> bstDelete x acc) t allElems
    assert (bstSize t3 == 0) "all deleted"
    assert (t3 == BSTLeaf) "is leaf"
    putStrLn "ok"
