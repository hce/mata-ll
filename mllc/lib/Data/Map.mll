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
--
-- Import this module `qualified`, as in Haskell:
--     import qualified Data.Map as M
-- Several names here (map, filter, null, lookup) deliberately shadow the
-- Prelude; because mata-ll flattens all imports into one namespace, an
-- unqualified `import Data.Map` would collide with the Prelude versions.
-- Qualification keeps both usable.
--
-- Keys must be primitive (String or a numeric type): the backing Lua
-- table hashes compound keys by identity, and the ordering operations
-- (keys/values/toList) sort keys, which only works for comparable
-- primitives. See doc/articles/SPEC.md for the rationale.

-- Map is an alias for HashMap
type Map k v = HashMap k v

empty :: Map k v
empty = hmEmpty

singleton :: Hashable k => k -> v -> Map k v
singleton k v = hmInsert k v hmEmpty

insert :: Hashable k => k -> v -> Map k v -> Map k v
insert = hmInsert

delete :: Hashable k => k -> Map k v -> Map k v
delete = hmDelete

lookup :: Hashable k => k -> Map k v -> Maybe v
lookup = hmLookup

member :: Hashable k => k -> Map k v -> Bool
member = hmMember

size :: Map k v -> Int
size = hmSize

keys :: Map k v -> [k]
keys = hmKeys

values :: Map k v -> [v]
values = hmValues

toList :: Map k v -> [(k, v)]
toList = hmToList

fromList :: Hashable k => [(k, v)] -> Map k v
fromList = hmFromList

map :: Hashable k => (v -> w) -> Map k v -> Map k w
map f m = fromList (mapPairs f (toList m))

mapPairs :: (v -> w) -> [(k, v)] -> [(k, w)]
mapPairs _ [] = []
mapPairs f ((k, v):rest) = (k, f v) : mapPairs f rest

filter :: Hashable k => (v -> Bool) -> Map k v -> Map k v
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

union :: Hashable k => Map k v -> Map k v -> Map k v
union m1 m2 = fromList (listAppend (toList m2) (toList m1))

listAppend :: [a] -> [a] -> [a]
listAppend [] ys = ys
listAppend (x:xs) ys = x : listAppend xs ys

intersection :: (Eq k, Hashable k) => Map k v -> Map k v -> Map k v
intersection m1 m2 = filter (\_ -> True) (fromList (filterByKeys (keys m2) (toList m1)))

filterByKeys :: Eq k => [k] -> [(k, v)] -> [(k, v)]
filterByKeys _ [] = []
filterByKeys ks ((k, v):rest) = if elem k ks then (k, v) : filterByKeys ks rest else filterByKeys ks rest

difference :: (Eq k, Hashable k) => Map k v -> Map k v -> Map k v
difference m1 m2 = fromList (filterNotByKeys (keys m2) (toList m1))

filterNotByKeys :: Eq k => [k] -> [(k, v)] -> [(k, v)]
filterNotByKeys _ [] = []
filterNotByKeys ks ((k, v):rest) = if elem k ks then filterNotByKeys ks rest else (k, v) : filterNotByKeys ks rest

null :: Map k v -> Bool
null m = size m == 0
