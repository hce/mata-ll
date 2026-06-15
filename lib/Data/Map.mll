module Data.Map
    ( Map
    , empty, singleton, insert, delete
    , lookup, member, size
    , keys, values, toList, fromList
    , map, filter, foldlWithKey, foldrWithKey
    , union, intersection, difference
    , null
    ) where

-- Data.Map is backed by the HashMap FFI type.
-- We re-export under the Haskell-compatible API.

-- Map is an alias for HashMap
type Map k v = HashMap k v

empty :: Map k v
empty = hmEmpty

singleton :: k -> v -> Map k v
singleton k v = hmInsert k v hmEmpty

insert :: k -> v -> Map k v -> Map k v
insert = hmInsert

delete :: k -> Map k v -> Map k v
delete = hmDelete

lookup :: k -> Map k v -> Maybe v
lookup = hmLookup

member :: k -> Map k v -> Bool
member = hmMember

size :: Map k v -> Integer
size = hmSize

keys :: Map k v -> [k]
keys = hmKeys

values :: Map k v -> [v]
values = hmValues

toList :: Map k v -> [(k, v)]
toList m = zip (keys m) (values m)

fromList :: [(k, v)] -> Map k v
fromList = hmFromList

map :: (v -> w) -> Map k v -> Map k w
map f m = fromList (mapPairs f (toList m))

mapPairs :: (v -> w) -> [(k, v)] -> [(k, w)]
mapPairs _ [] = []
mapPairs f ((k, v):rest) = (k, f v) : mapPairs f rest

filter :: (v -> Bool) -> Map k v -> Map k v
filter p m = fromList (filterPairs p (toList m))

filterPairs :: (v -> Bool) -> [(k, v)] -> [(k, v)]
filterPairs _ [] = []
filterPairs p ((k, v):rest) = if p v then (k, v) : filterPairs p rest else filterPairs p rest

foldlWithKey :: (b -> k -> v -> b) -> b -> Map k v -> b
foldlWithKey f acc m = foldlPairs f acc (toList m)

foldlPairs :: (b -> k -> v -> b) -> b -> [(k, v)] -> b
foldlPairs _ acc [] = acc
foldlPairs f acc ((k, v):rest) = foldlPairs f (f acc k v) rest

foldrWithKey :: (k -> v -> b -> b) -> b -> Map k v -> b
foldrWithKey f acc m = foldrPairs f acc (toList m)

foldrPairs :: (k -> v -> b -> b) -> b -> [(k, v)] -> b
foldrPairs _ acc [] = acc
foldrPairs f acc ((k, v):rest) = f k v (foldrPairs f acc rest)

union :: Map k v -> Map k v -> Map k v
union m1 m2 = fromList (listAppend (toList m2) (toList m1))

listAppend :: [a] -> [a] -> [a]
listAppend [] ys = ys
listAppend (x:xs) ys = x : listAppend xs ys

intersection :: Map k v -> Map k v -> Map k v
intersection m1 m2 = filter (\_ -> True) (fromList (filterByKeys (keys m2) (toList m1)))

filterByKeys :: [k] -> [(k, v)] -> [(k, v)]
filterByKeys _ [] = []
filterByKeys ks ((k, v):rest) = if elem k ks then (k, v) : filterByKeys ks rest else filterByKeys ks rest

difference :: Map k v -> Map k v -> Map k v
difference m1 m2 = fromList (filterNotByKeys (keys m2) (toList m1))

filterNotByKeys :: [k] -> [(k, v)] -> [(k, v)]
filterNotByKeys _ [] = []
filterNotByKeys ks ((k, v):rest) = if elem k ks then filterNotByKeys ks rest else (k, v) : filterNotByKeys ks rest

null :: Map k v -> Bool
null m = size m == 0
