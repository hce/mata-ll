-- GHC cgrun060: Simple key-value trie using Integer keys

data Trie = TrieNode Bool [(Integer, Trie)]
    deriving (Eq)

emptyTrie :: Trie
emptyTrie = TrieNode False []

trieInsert :: [Integer] -> Trie -> Trie
trieInsert [] (TrieNode _ children) = TrieNode True children
trieInsert (k:ks) (TrieNode end children) =
    case findChild k children of
        Nothing  -> TrieNode end ((k, trieInsert ks emptyTrie) : children)
        Just sub -> TrieNode end (updateChild k (trieInsert ks sub) children)

findChild :: Integer -> [(Integer, Trie)] -> Maybe Trie
findChild _ [] = Nothing
findChild k ((k2, v):rest)
    | k == k2   = Just v
    | otherwise  = findChild k rest

updateChild :: Integer -> Trie -> [(Integer, Trie)] -> [(Integer, Trie)]
updateChild _ _ [] = []
updateChild k v ((k2, v2):rest)
    | k == k2   = (k, v) : rest
    | otherwise  = (k2, v2) : updateChild k v rest

trieSearch :: [Integer] -> Trie -> Bool
trieSearch [] (TrieNode end _) = end
trieSearch (k:ks) (TrieNode _ children) =
    case findChild k children of
        Nothing  -> False
        Just sub -> trieSearch ks sub

main :: IO ()
main = do
    let t0 = emptyTrie
    let t1 = trieInsert [1, 2, 3] t0
    let t2 = trieInsert [1, 2, 4] t1
    let t3 = trieInsert [2, 3] t2

    assert (trieSearch [1, 2, 3] t3) "find 1,2,3"
    assert (trieSearch [1, 2, 4] t3) "find 1,2,4"
    assert (trieSearch [2, 3] t3) "find 2,3"
    assert (not (trieSearch [1, 2] t3)) "1,2 not end"
    assert (not (trieSearch [1, 2, 5] t3)) "1,2,5 not found"
    assert (not (trieSearch [3] t3)) "3 not found"
    assert (not (trieSearch [] t3)) "empty not found"
    assert (not (trieSearch [1] emptyTrie)) "empty trie"
    putStrLn "ok"
