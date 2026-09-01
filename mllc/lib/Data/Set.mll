-- Data.Set (A20): ordered sets over the structural-key HashMap machinery.
-- A Set is a HashMap to Bool (True per present element — NOT to (): the
-- unit value's runtime representation is Lua nil, and a nil table value
-- DELETES its key): scalar elements key the Lua table directly,
-- structural elements (tuples, lists, Maybe) go through the A17 encoded
-- entries, and toList enumerates in ascending Ord order (the A16
-- structural compare) either way. Implemented directly over the hm*
-- builtins — not via qualified Data.Map — to stay clear of the nested
-- qualified-import seam (Q77). Like Data.Map, this is mata-ll's own
-- library with containers-compatible names, not GHC's containers.
module Data.Set
    ( Set, empty, singleton, insert, delete, member, size, null
    , fromList, toList, union, intersection, difference, filter
    ) where

type Set a = HashMap a Bool

empty :: Set a
empty = hmEmpty

singleton :: Hashable a => a -> Set a
singleton x = hmInsert x True hmEmpty

insert :: Hashable a => a -> Set a -> Set a
insert x s = hmInsert x True s

delete :: Hashable a => a -> Set a -> Set a
delete x s = hmDelete x s

member :: Hashable a => a -> Set a -> Bool
member x s = hmMember x s

size :: Set a -> Int
size = hmSize

null :: Set a -> Bool
null s = hmSize s == 0

fromList :: Hashable a => [a] -> Set a
fromList [] = hmEmpty
fromList (x : xs) = insert x (fromList xs)

-- Ascending element order (scalars natively, structural elements by the
-- A16 compare).
toList :: Set a -> [a]
toList s = hmKeys s

union :: Hashable a => Set a -> Set a -> Set a
union s1 s2 = unionGo (toList s1) s2

unionGo :: Hashable a => [a] -> Set a -> Set a
unionGo [] s = s
unionGo (x : xs) s = insert x (unionGo xs s)

-- intersection/difference probe the second set per element (a hash
-- lookup), O(n) overall — the Data.Map F15 discipline.
intersection :: Hashable a => Set a -> Set a -> Set a
intersection s1 s2 = fromList (keep s2 (toList s1))

keep :: Hashable a => Set a -> [a] -> [a]
keep _ [] = []
keep s (x : xs) = if member x s then x : keep s xs else keep s xs

difference :: Hashable a => Set a -> Set a -> Set a
difference s1 s2 = fromList (dropIn s2 (toList s1))

dropIn :: Hashable a => Set a -> [a] -> [a]
dropIn _ [] = []
dropIn s (x : xs) = if member x s then dropIn s xs else x : dropIn s xs

filter :: Hashable a => (a -> Bool) -> Set a -> Set a
filter p s = fromList (keepP p (toList s))

keepP :: (a -> Bool) -> [a] -> [a]
keepP _ [] = []
keepP p (x : xs) = if p x then x : keepP p xs else keepP p xs
