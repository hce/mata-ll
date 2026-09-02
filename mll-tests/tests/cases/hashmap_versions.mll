-- Persistent-map versioning: maps are diff+reroot handles at runtime
-- (one live store per version family; old handles replay onto it when
-- read). Everything here is observational — these asserts pin that the
-- representation trick never leaks: every version answers as if it were
-- an independent copy, in every read order.

fromMaybe :: a -> Maybe a -> a
fromMaybe def Nothing = def
fromMaybe _ (Just x) = x

pairsEq :: [(String, Int)] -> [(String, Int)] -> Bool
pairsEq [] [] = True
pairsEq ((k1, v1) : rest1) ((k2, v2) : rest2) = k1 == k2 && v1 == v2 && pairsEq rest1 rest2
pairsEq _ _ = False

-- A chain of n inserts on top of m: keys "c1".."cn" (distinct from any
-- base key), so the diff chain from m to the newest version has length n.
chain :: Int -> HashMap String Int -> HashMap String Int
chain 0 m = m
chain n m = hmInsert ("c" <> show n) n (chain (n - 1) m)

main :: IO ()
main = do
    -- Linear history, read backward then forward then middle.
    let m0 = hmFromList [("a", 1), ("b", 2)]
    let m1 = hmInsert "c" 3 m0
    let m2 = hmDelete "a" m1
    -- Newest first (m2 is the natural root)…
    assert (hmSize m2 == 2) "m2 size"
    assert (not (hmMember "a" m2)) "a deleted in m2"
    -- …then the OLDEST (reroot walks the whole chain backward)…
    assert (hmSize m0 == 2) "m0 size after later versions"
    assert (fromMaybe 0 (hmLookup "a" m0) == 1) "m0 keeps a"
    assert (not (hmMember "c" m0)) "m0 has no c"
    -- …then forward again to the newest, then the middle version.
    assert (fromMaybe 0 (hmLookup "c" m2) == 3) "m2 keeps c"
    assert (hmSize m1 == 3) "m1 size"
    assert (fromMaybe 0 (hmLookup "a" m1) == 1) "m1 keeps a"
    assert (pairsEq (hmToList m0) [("a", 1), ("b", 2)]) "m0 toList intact"
    assert (pairsEq (hmToList m2) [("b", 2), ("c", 3)]) "m2 toList intact"

    -- Overwrite records the OLD value in the diff: the old version must
    -- read it back after the store moved on.
    let o1 = hmInsert "a" 10 m0
    assert (fromMaybe 0 (hmLookup "a" o1) == 10) "overwrite visible"
    assert (fromMaybe 0 (hmLookup "a" m0) == 1) "old value restored on reroot"
    assert (hmSize o1 == 2) "overwrite keeps size"

    -- Fork: two futures from one base; all three answer independently.
    let fA = hmInsert "x" 7 m0
    let fB = hmDelete "b" m0
    assert (hmSize fA == 3 && hmSize fB == 1 && hmSize m0 == 2) "fork sizes"
    assert (hmMember "x" fA && not (hmMember "x" fB) && not (hmMember "x" m0)) "fork x"
    assert (fromMaybe 0 (hmLookup "b" fA) == 2) "fork A keeps b"

    -- Stored Nothing (the nil-sentinel) survives flips in both directions.
    let s0 = hmFromList [("k", Nothing), ("l", Just 5)]
    let s1 = hmDelete "k" s0
    let s2 = hmInsert "k" (Just 6) s1
    assert (hmSize s2 == 2) "sentinel chain size"
    assert (hmMember "k" s0) "stored Nothing is present in s0"
    assert (fromMaybe (Just 99) (hmLookup "k" s0) == Nothing) "stored Nothing reads back"
    assert (not (hmMember "k" s1)) "deleted in s1"
    assert (fromMaybe Nothing (hmLookup "k" s2) == Just 6) "reinserted in s2"

    -- Delete of an absent key returns the map unchanged (no version made).
    let d = hmDelete "zz" m0
    assert (hmSize d == 2 && fromMaybe 0 (hmLookup "a" d) == 1) "delete absent"

    -- A chain far past the reroot cap: reading the base MATERIALIZES a
    -- fresh store instead of replaying; both ends stay correct, and
    -- re-reading the newest afterwards still answers.
    let big = chain 40 m0
    assert (hmSize big == 42) "long chain size"
    assert (hmSize m0 == 2) "base after long chain"
    assert (fromMaybe 0 (hmLookup "b" m0) == 2) "base b after materialize"
    assert (fromMaybe 0 (hmLookup "c40" big) == 40) "chain head after base read"
    assert (fromMaybe 0 (hmLookup "a" big) == 1) "chain keeps base keys"

    -- The shared empty is frozen: building from it never disturbs it or
    -- any sibling build.
    let e1 = hmInsert "p" 1 hmEmpty
    let e2 = hmInsert "q" 2 hmEmpty
    assert (hmSize (hmEmpty :: HashMap String Int) == 0) "empty stays empty"
    assert (hmSize e1 == 1 && hmSize e2 == 1) "sibling builds from empty"
    assert (not (hmMember "p" e2) && not (hmMember "q" e1)) "no cross-talk"

    -- Structural (encoded) keys: the same versioning through the hme
    -- family, keys back in Ord order on every version.
    let t0 = hmFromList [((1, "u"), 10), ((2, "v"), 20)]
    let t1 = hmInsert (3, "w") 30 t0
    let t2 = hmDelete (1, "u") t1
    assert (hmSize t2 == 2) "hme t2 size"
    assert (hmSize t0 == 2) "hme t0 size after versions"
    assert (fromMaybe 0 (hmLookup (1, "u") t0) == 10) "hme old version keeps key"
    assert (not (hmMember (1, "u") t2)) "hme delete visible"
    assert (fromMaybe 0 (hmLookup (3, "w") t1) == 30) "hme middle version"
    assert (hmKeys t0 == [(1, "u"), (2, "v")]) "hme t0 keys ordered"
    assert (hmKeys t2 == [(2, "v"), (3, "w")]) "hme t2 keys ordered"

    putStrLn "versions ok"
