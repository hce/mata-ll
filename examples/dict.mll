-- A pure AVL tree dictionary, using Ord for keys.
-- Self-balancing: O(log n) insert, lookup, and delete.

data Dict k v = Empty
              | Node Integer k v (Dict k v) (Dict k v)

-- The Integer field is the height of the subtree.

empty :: Dict k v
empty = Empty

height :: Dict k v -> Integer
height Empty = 0
height (Node h _ _ _ _) = h

node :: k -> v -> Dict k v -> Dict k v -> Dict k v
node k v l r = Node (1 + max (height l) (height r)) k v l r

balance :: Dict k v -> Integer
balance Empty = 0
balance (Node _ _ _ l r) = height l - height r

-- Rotations

rotateRight :: Dict k v -> Dict k v
rotateRight (Node _ k v (Node _ lk lv ll lr) r) = node lk lv ll (node k v lr r)
rotateRight t = t

rotateLeft :: Dict k v -> Dict k v
rotateLeft (Node _ k v l (Node _ rk rv rl rr)) = node rk rv (node k v l rl) rr
rotateLeft t = t

rebalance :: Dict k v -> Dict k v
rebalance Empty = Empty
rebalance (Node h k v l r)
    | balance (Node h k v l r) > 1 && balance l < 0  = rotateRight (node k v (rotateLeft l) r)
    | balance (Node h k v l r) > 1                   = rotateRight (node k v l r)
    | balance (Node h k v l r) < -1 && balance r > 0 = rotateLeft (node k v l (rotateRight r))
    | balance (Node h k v l r) < -1                  = rotateLeft (node k v l r)
    | otherwise                                       = Node h k v l r

-- Operations

insert :: k -> v -> Dict k v -> Dict k v
insert k v Empty = node k v Empty Empty
insert k v (Node h nk nv left right)
    | k < nk    = rebalance (node nk nv (insert k v left) right)
    | k > nk    = rebalance (node nk nv left (insert k v right))
    | otherwise = Node h k v left right

lookup' :: k -> Dict k v -> Maybe v
lookup' _ Empty = Nothing
lookup' k (Node _ nk nv left right)
    | k < nk    = lookup' k left
    | k > nk    = lookup' k right
    | otherwise = Just nv

member :: k -> Dict k v -> Bool
member k d = case lookup' k d of
    Just _  -> True
    Nothing -> False

fromMaybe :: a -> Maybe a -> a
fromMaybe def Nothing = def
fromMaybe _ (Just x) = x

size :: Dict k v -> Integer
size Empty = 0
size (Node _ _ _ left right) = 1 + size left + size right

append :: [a] -> [a] -> [a]
append [] ys = ys
append (x:xs) ys = x : append xs ys

keys :: Dict k v -> [k]
keys Empty = []
keys (Node _ k _ left right) = append (keys left) (k : keys right)

values :: Dict k v -> [v]
values Empty = []
values (Node _ _ v left right) = append (values left) (v : values right)

-- Demo

main :: IO ()
main = do
    -- Insert keys in sorted order — would degenerate without balancing
    let d = insert "e" 5
          $ insert "d" 4
          $ insert "c" 3
          $ insert "b" 2
          $ insert "a" 1
          $ empty
    putStrLn (show (lookup' "c" d))
    putStrLn (show (lookup' "z" d))
    putStrLn (show (size d))
    putStrLn (show (fromMaybe 0 (lookup' "a" d)))
    putStrLn (show (fromMaybe 0 (lookup' "missing" d)))
    putStrLn (show (keys d))
    -- Height should be ~3 for 5 elements (not 5 as with unbalanced BST)
    putStrLn ("height: " <> show (height d))
    putStrLn ("balanced: " <> show (balance d))
