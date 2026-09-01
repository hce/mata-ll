module Data.Map
    ( Map
    , empty, singleton, insert, delete
    , lookup, member, size
    , keys, values, toList, fromList
    , map, filter, foldlWithKey, foldrWithKey
    , union, intersection, difference
    , null
    , elems, findWithDefault, insertWith, adjust
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
insert k v m = hmInsert k v m

delete :: Hashable k => k -> Map k v -> Map k v
delete k m = hmDelete k m

lookup :: Hashable k => k -> Map k v -> Maybe v
lookup k m = hmLookup k m

member :: Hashable k => k -> Map k v -> Bool
member k m = hmMember k m

size :: Map k v -> Int
size = hmSize

keys :: Map k v -> [k]
keys m = hmKeys m

values :: Map k v -> [v]
values m = hmValues m

toList :: Map k v -> [(k, v)]
toList m = hmToList m

fromList :: Hashable k => [(k, v)] -> Map k v
fromList kvs = hmFromList kvs

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

-- intersection/difference probe the SECOND map directly (hmMember, a hash
-- lookup) per entry of the first — O(n) overall. The previous spelling
-- materialized `keys m2` and ran `elem` per entry, O(n*m) (F15).
intersection :: (Eq k, Hashable k) => Map k v -> Map k v -> Map k v
intersection m1 m2 = fromList (keepMember m2 (toList m1))

keepMember :: Hashable k => Map k v -> [(k, v)] -> [(k, v)]
keepMember _ [] = []
keepMember m ((k, v):rest) = if member k m then (k, v) : keepMember m rest else keepMember m rest

difference :: (Eq k, Hashable k) => Map k v -> Map k v -> Map k v
difference m1 m2 = fromList (dropMember m2 (toList m1))

dropMember :: Hashable k => Map k v -> [(k, v)] -> [(k, v)]
dropMember _ [] = []
dropMember m ((k, v):rest) = if member k m then dropMember m rest else (k, v) : dropMember m rest

null :: Map k v -> Bool
null m = size m == 0

-- containers-compatible additions (A20).
elems :: Map k v -> [v]
elems = values

findWithDefault :: Hashable k => v -> k -> Map k v -> v
findWithDefault d k m = case lookup k m of
    Nothing -> d
    Just v  -> v

insertWith :: Hashable k => (v -> v -> v) -> k -> v -> Map k v -> Map k v
insertWith f k v m = case lookup k m of
    Nothing  -> insert k v m
    Just old -> insert k (f v old) m

adjust :: Hashable k => (v -> v) -> k -> Map k v -> Map k v
adjust f k m = case lookup k m of
    Nothing -> m
    Just v  -> insert k (f v) m
