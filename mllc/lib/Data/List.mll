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
drop 0 xs = xs
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

sortBy :: (a -> a -> Ordering) -> [a] -> [a]
sortBy _ [] = []
sortBy cmp (x:xs) = let less = filter (\y -> cmp y x == LT || cmp y x == EQ) xs
                        greater = filter (\y -> cmp y x == GT) xs
                    in append (sortBy cmp less) (x : sortBy cmp greater)

foldl' :: (b -> a -> b) -> b -> [a] -> b
foldl' _ acc [] = acc
foldl' f acc (x:xs) = seq (f acc x) (foldl' f (f acc x) xs)
