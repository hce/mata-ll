module Data.List
    ( map, filter, foldl, foldr, foldl'
    , head, tail, last, init
    , null, length, reverse
    , concat, concatMap
    , take, drop, takeWhile, dropWhile
    , span, break'
    , zip, unzip, zipWith
    , sortBy
    , nubBy
    , groupBy
    , intersperse, intercalate
    , partition
    , replicate
    , iterate
    , unfoldr
    , scanl, scanr
    , sum, product
    , and, or
    , any, all
    , find
    , append
    ) where

-- Most list functions now live in the auto-imported Prelude (null, last, init,
-- concat, span, zip, unzip, replicate, iterate, sum, product, and, or, any,
-- all, takeWhile, dropWhile, ...). They are re-exported above so existing
-- `import Data.List (...)` selections keep working. This module defines only
-- the helpers that are NOT in the Prelude.

-- List append (++ is the built-in operator; this is the named version).
append :: [a] -> [a] -> [a]
append [] ys = ys
append (x:xs) ys = x : append xs ys

drop :: Int -> [a] -> [a]
-- GHC returns the whole list for n <= 0 (the runtime's native drop
-- already does); matching only 0 recursed a negative n right through
-- every element to [].
drop n xs | n <= 0 = xs
drop _ [] = []
drop n (_:xs) = drop (n - 1) xs

break' :: (a -> Bool) -> [a] -> ([a], [a])
break' p = span (\x -> not (p x))

nubBy :: (a -> a -> Bool) -> [a] -> [a]
nubBy _ [] = []
nubBy eq (x:xs) = x : nubBy eq (filter (\y -> not (eq x y)) xs)

groupBy :: (a -> a -> Bool) -> [a] -> [[a]]
groupBy _ [] = []
groupBy eq (x:xs) = let sp = span (eq x) xs
                     in (x : fst sp) : groupBy eq (snd sp)

intersperse :: a -> [a] -> [a]
intersperse _ [] = []
intersperse _ [x] = [x]
intersperse sep (x:xs) = x : sep : intersperse sep xs

intercalate :: [a] -> [[a]] -> [a]
intercalate sep xs = concat (intersperse sep xs)

partition :: (a -> Bool) -> [a] -> ([a], [a])
partition _ [] = ([], [])
partition p (x:xs) = case partition p xs of
    (yes, no) -> if p x then (x : yes, no) else (yes, x : no)

unfoldr :: (b -> Maybe (a, b)) -> b -> [a]
unfoldr f b = case f b of
    Nothing -> []
    Just (a, b2) -> a : unfoldr f b2

scanl :: (b -> a -> b) -> b -> [a] -> [b]
scanl _ acc [] = [acc]
scanl f acc (x:xs) = acc : scanl f (f acc x) xs

scanr :: (a -> b -> b) -> b -> [a] -> [b]
scanr _ acc [] = [acc]
scanr f acc (x:xs) = let rest = scanr f acc xs
                     in f x (head rest) : rest

find :: (a -> Bool) -> [a] -> Maybe a
find _ [] = Nothing
find p (x:xs) = if p x then Just x else find p xs

-- Stable bottom-up merge sort, like GHC's Data.List.sortBy: elements
-- comparing EQ keep their input order (merge takes from the LEFT run on
-- EQ), each comparison calls cmp once, and the pass structure is
-- O(n log n) on every input.  (The previous quicksort was unstable —
-- EQ-to-pivot elements moved in front of the pivot — quadratic on
-- sorted input, and called cmp up to three times per element.)
sortBy :: (a -> a -> Ordering) -> [a] -> [a]
sortBy cmp list = go (map (\x -> [x]) list)
  where
    go [] = []
    go (r : rs) = case rs of
        [] -> r
        _ -> go (mergePairs (r : rs))
    mergePairs (a : b : rest) = merge a b : mergePairs rest
    mergePairs rs = rs
    merge [] bs = bs
    merge (a : rest) [] = a : rest
    merge (a : as) (b : bs) = if cmp a b == GT
        then b : merge (a : as) bs
        else a : merge as (b : bs)

-- foldl' is a Foldable class method (the Prelude's, as in GHC 9.10+ where
-- the Prelude exports it); re-exported above for `import Data.List
-- (foldl')`.
