-- H1: a map READ whose key is evaluated while the read runs. Encoding a
-- structural key forces its components, and a component that reads
-- another version of the same map family reroots the shared store; a
-- reader that had fetched the store before encoding the key answered for
-- that other version (`M.lookup (2, M.size m2) m1` found m2's entry).
-- Every reader family with a key that reads a sibling version, in both
-- directions (the newer version's key read from the older map and the
-- reverse): structural keys (tuple, list, Maybe, nested), scalar and
-- Number keys, the dictionary-routed world (an existential Hashable
-- key), Data.Set, and the Data.Map readers that go through lookup
-- (findWithDefault, insertWith, adjust). Goldened against GHC's Data.Map.
module Main where

import qualified Data.Map as M
import qualified Data.Set as S

data Box = forall k. Hashable k => Box k (M.Map k String)

boxedLookup :: Box -> Maybe String
boxedLookup (Box k m) = M.lookup k m

boxedMember :: Box -> Bool
boxedMember (Box k m) = M.member k m

probe :: Hashable k => k -> M.Map k String -> (Maybe String, Bool)
probe k m = (M.lookup k m, M.member k m)

classify :: Maybe String -> String
classify (Just v) = "hit " <> v
classify Nothing = "miss"

main :: IO ()
main = do
    -- The older version is read with a key whose component reads the newer one.
    let m1 = M.fromList [((1, 1), "a")] :: M.Map (Int, Int) String
        m2 = M.insert (2, 2) "b" m1
    print (M.lookup (2, M.size m2) m1)
    print (M.member (2, M.size m2) m1)
    print (M.lookup (M.size m2, 2) m1)
    print (M.findWithDefault "none" (2, M.size m2) m1)
    print (M.toList (M.insertWith (<>) (2, M.size m2) "z" m1))
    print (M.toList (M.adjust (<> "!") (2, M.size m2) m1))
    print (M.lookup (1, M.size m2 - 1) m1)
    -- The newer version is read with a key whose component reads the older one.
    let n1 = M.fromList [((1, 1), "a"), ((2, 2), "b")] :: M.Map (Int, Int) String
        n2 = M.delete (2, 2) n1
    print (M.size n2)
    print (M.lookup (2, M.size n1) n2)
    print (M.member (2, M.size n1) n2)
    print (M.lookup (1, M.size n1 - 1) n2)
    -- A component that is itself a lookup in the sibling version.
    let p1 = M.fromList [((1, 1), 1)] :: M.Map (Int, Int) Int
        p2 = M.insert (2, 2) 2 p1
    print (M.lookup (2, M.findWithDefault 0 (2, 2) p2) p1)
    -- List, Maybe and nested structural keys.
    let l1 = M.fromList [([1], "a")] :: M.Map [Int] String
        l2 = M.insert [2] "b" l1
    print (M.lookup [M.size l2] l1, M.member [M.size l2] l1)
    let j1 = M.fromList [(Just 1, "a")] :: M.Map (Maybe Int) String
        j2 = M.insert (Just 2) "b" j1
    print (M.lookup (Just (M.size j2)) j1, M.member (Just (M.size j2)) j1)
    let q1 = M.fromList [(((1, "x"), [True]), "a")] :: M.Map ((Int, String), [Bool]) String
        q2 = M.insert ((2, "y"), [False]) "b" q1
    print (M.lookup ((M.size q2, "y"), [False]) q1)
    print (M.lookup ((2, show (M.size q2)), [even (M.size q2)]) (M.insert ((2, "2"), [True]) "c" q2))
    -- Scalar keys: the key is forced before the store is touched.
    let s1 = M.fromList [(1, "a")] :: M.Map Int String
        s2 = M.insert 2 "b" s1
    print (M.lookup (M.size s2) s1, M.member (M.size s2) s1)
    print (case M.lookup (M.size s2) s1 of
        Just v -> "hit " <> v
        Nothing -> "miss")
    print (classify (M.lookup (M.size s2 - 1) s1))
    let t1 = M.fromList [("1", "a")] :: M.Map String String
        t2 = M.insert "2" "b" t1
    print (M.lookup (show (M.size t2)) t1, M.member (show (M.size t2)) t1)
    let b1 = M.fromList [(False, "a")] :: M.Map Bool String
        b2 = M.insert True "b" b1
    print (M.lookup (even (M.size b2)) b1, M.member (even (M.size b2)) b1)
    -- Number keys, scalar and structural.
    let d1 = M.fromList [(1, "a")] :: M.Map Number String
        d2 = M.insert 2 "b" d1
    print (M.lookup (fromIntegral (M.size d2)) d1, M.member (fromIntegral (M.size d2)) d1)
    let e1 = M.fromList [((1, 1), "a")] :: M.Map (Number, Number) String
        e2 = M.insert (2, 2) "b" e1
    print (M.lookup (2, fromIntegral (M.size e2)) e1, M.member (2, fromIntegral (M.size e2)) e1)
    -- A Hashable-constrained body at a structural key type.
    print (probe (2, M.size m2) m1)
    print (probe (2, M.size n1) n2)
    print (probe [M.size l2] l1)
    -- The dictionary-routed world: the key type is hidden in an existential.
    print (boxedLookup (Box (2, M.size m2) m1))
    print (boxedMember (Box (2, M.size m2) m1))
    print (boxedLookup (Box (2, M.size n1) n2))
    print (boxedLookup (Box [M.size l2] l1))
    print (boxedLookup (Box (Just (M.size j2)) j1))
    print (boxedLookup (Box (M.size s2) s1))
    print (boxedLookup (Box (show (M.size t2)) t1))
    -- Data.Set.
    let x1 = S.fromList [(1, 1)] :: S.Set (Int, Int)
        x2 = S.insert (2, 2) x1
    print (S.member (2, S.size x2) x1)
    print (S.member (2, S.size x2) x2)
    print (S.toList x1, S.toList x2)
    -- Every version still answers for itself afterwards.
    print (M.toList m1, M.toList m2, M.toList n1, M.toList n2)
