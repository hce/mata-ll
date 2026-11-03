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

-- GHC's definitions: `intersperse` emits the first element and hands the
-- rest to `prependToAll`, which puts the separator before every further
-- element. An `[x]` clause would have to match the TAIL before producing
-- `x`, so `take 1 (intersperse 0 (1 : ⊥))` raised where GHC gives `[1]`.
intersperse :: a -> [a] -> [a]
intersperse _ [] = []
intersperse sep (x:xs) = x : prependToAll sep xs

prependToAll :: a -> [a] -> [a]
prependToAll _ [] = []
prependToAll sep (x:xs) = sep : x : prependToAll sep xs

intercalate :: [a] -> [[a]] -> [a]
intercalate sep xs = concat (intersperse sep xs)

-- GHC's partition is a foldr whose step decides `p x` and passes the
-- other component through lazily (`~(ts, fs)`); each component is then
-- `filter` of the list by the shared decisions. Here the two components
-- are those filters over the list tagged with its decisions (`p` runs once
-- per element and both sides share the result), so each side is walked
-- in constant stack however long a run of elements goes to the other
-- side; the pass-through selector chain of the literal foldr would be
-- forced one nested call per such element. The pair itself is defined
-- exactly when GHC's is: on the list's first cell and its decision.
partition :: (a -> Bool) -> [a] -> ([a], [a])
partition p xs = case tagged of
    [] -> ([], [])
    ((b, _) : _) -> seq b (pick True, pick False)
  where
    tagged = map (\x -> (p x, x)) xs
    pick want = map snd (filter (\t -> fst t == want) tagged)

unfoldr :: (b -> Maybe (a, b)) -> b -> [a]
unfoldr f b = case f b of
    Nothing -> []
    Just (a, b2) -> a : unfoldr f b2

-- scanl/scanr live in the Prelude (as in GHC) and are re-exported above.

find :: (a -> Bool) -> [a] -> Maybe a
find _ [] = Nothing
find p (x:xs) = if p x then Just x else find p xs

-- sortBy lives in the Prelude (GHC 9.14.1's Data.OldList algorithm, ported
-- clause for clause) and is re-exported above.

-- foldl' is a Foldable class method (the Prelude's, as in GHC 9.10+ where
-- the Prelude exports it); re-exported above for `import Data.List
-- (foldl')`.
