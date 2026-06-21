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

-- Most list functions are already in the Prelude.
-- We add missing ones here.

-- List append (++ is the built-in operator, this is the named version)
append :: [a] -> [a] -> [a]
append [] ys = ys
append (x:xs) ys = x : append xs ys

null :: [a] -> Bool
null [] = True
null _ = False

last :: [a] -> a
last [x] = x
last (_:xs) = last xs
last [] = error "Data.List.last: empty list"

init :: [a] -> [a]
init [_] = []
init (x:xs) = x : init xs
init [] = error "Data.List.init: empty list"

concat :: [[a]] -> [a]
concat [] = []
concat (xs:xss) = append xs (concat xss)

drop :: Integer -> [a] -> [a]
drop 0 xs = xs
drop _ [] = []
drop n (_:xs) = drop (n - 1) xs

takeWhile :: (a -> Bool) -> [a] -> [a]
takeWhile _ [] = []
takeWhile p (x:xs) = if p x then x : takeWhile p xs else []

dropWhile :: (a -> Bool) -> [a] -> [a]
dropWhile _ [] = []
dropWhile p (x:xs) = if p x then dropWhile p xs else x : xs

span :: (a -> Bool) -> [a] -> ([a], [a])
span _ [] = ([], [])
span p (x:xs) = if p x
    then let rest = span p xs in (x : fst rest, snd rest)
    else ([], x : xs)

break' :: (a -> Bool) -> [a] -> ([a], [a])
break' p = span (\x -> not (p x))

zip :: [a] -> [b] -> [(a, b)]
zip [] _ = []
zip _ [] = []
zip (a:as') (b:bs) = (a, b) : zip as' bs

unzip :: [(a, b)] -> ([a], [b])
unzip [] = ([], [])
unzip ((a, b):rest) = let r = unzip rest in (a : fst r, b : snd r)

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
partition p (x:xs) = let r = partition p xs
                     in if p x then (x : fst r, snd r) else (fst r, x : snd r)

replicate :: Integer -> a -> [a]
replicate 0 _ = []
replicate n x = x : replicate (n - 1) x

iterate :: (a -> a) -> a -> [a]
iterate f x = x : iterate f (f x)

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

sum :: [Integer] -> Integer
sum = foldl (\acc x -> acc + x) 0

product :: [Integer] -> Integer
product = foldl (\acc x -> acc * x) 1

and :: [Bool] -> Bool
and [] = True
and (x:xs) = if x then and xs else False

or :: [Bool] -> Bool
or [] = False
or (x:xs) = if x then True else or xs

any :: (a -> Bool) -> [a] -> Bool
any _ [] = False
any p (x:xs) = if p x then True else any p xs

all :: (a -> Bool) -> [a] -> Bool
all _ [] = True
all p (x:xs) = if p x then all p xs else False

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
