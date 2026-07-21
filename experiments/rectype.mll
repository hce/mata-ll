-- mata-ll's String is opaque, not [Char], so we can't feed a string straight
-- into the list-based `trie`. strToInts turns it into a list of character codes.
import LString (strToInts)

data Tree a b = DeadTree | Leaf a b | Branch (Tree a b) (Tree a b)
  deriving (Show)

find :: Eq a => a -> [(a, b)] -> Maybe b
find a []          = Nothing
find a ((k, v):xs)
    | k == a       = Just v
    | otherwise    = find a xs

replace :: Eq k => k -> v -> [(k, v)] -> [(k, v)]
replace _ _ []          = []
replace k v ((k', v'):xs)
    | k == k'    = (k, v):xs
    | otherwise  = (k', v'):(replace k v xs)

count :: Eq a => [a] -> [(a, Integer)] -> [(a, Integer)]
count []     l = l
count (x:xs) l = count xs l'
  where
    l' = case find x l of
             Just count -> replace x (count + 1) l
             Nothing    -> (x, 1):l

-- Keys are unique here, so ordering by the key (fst) is a total order.
-- mata-ll has no Ord instance for tuples, hence comparing fst rather than the
-- whole pair; and lists concatenate with ++ (<> is for String only).
sort :: Ord a => [(a, Integer)] -> [(a, Integer)]
sort []       = []
sort (a:[])   = [a]
sort (a:b:[]) = if fst a > fst b then b:a:[] else a:b:[]
sort (x:xs)   = if fst x > fst y then xs' ++ [x] else x:xs'
                   where xs' = sort xs
                         y   = head xs'

trie :: (Eq a, Ord a) => [a] -> Tree a Integer
trie [] = DeadTree
trie l  = (makeTree . sort) (count l [])
  where
    makeTree ((k, cnt):[])              = Leaf k cnt
    makeTree ((k1, cnt1):(k2, cnt2):[]) = Branch (Leaf k1 cnt1) (Leaf k2 cnt2)
    makeTree ((k, cnt):rest)            = Branch (Leaf k cnt) (makeTree rest)

main :: IO ()
main = do
    print $ trie (strToInts "hello world how are you we are counting letteres herelah")
    print $ trie (strToInts "abcdefghijklmnopqrstuvwxyz")
