-- Enumerating a map version while FORCING its values (show, the FFI
-- marshallers) must not observe another version's contents. The diff+
-- reroot representation keeps one mutable store per version family; a
-- value that is a thunk reading ANOTHER version reroots that store when
-- forced. Walking the live store with `pairs` across such a force printed
-- the other version's keys (a silent wrong answer found by an isolated
-- review). Every forcing enumerator snapshots the entries first.
-- Expected values computed with a Data.Map twin under runghc.

main :: IO ()
main = do
    let m1 = hmFromList [(1, [0 :: Int]), (2, [0])] :: HashMap Int [Int]
        m2 = hmInsert 3 [0] m1
        -- m3's value for key 2 is a thunk that reads m2: forcing it during
        -- an enumeration of m3 reroots the family from m3 to m2.
        m3 = hmInsert 2 [hmSize m2] m1
    assert (show m3 == "{1 -> [0], 2 -> [3]}") ("show m3: " <> show m3)
    assert (hmSize m3 == 2) "m3 size"
    assert (hmKeys m3 == [1, 2]) "m3 keys"
    assert (show (hmToList m3) == "[(1,[0]),(2,[3])]") "m3 toList"
    -- Read the other version afterwards, then show m3 again (the family is
    -- now rooted at m2; show reroots back and snapshots).
    assert (hmSize m2 == 3) "m2 size"
    assert (show m3 == "{1 -> [0], 2 -> [3]}") "show m3 after m2 read"
    assert (show m2 == "{1 -> [0], 2 -> [0], 3 -> [0]}") "show m2"
    -- Structural-key family, same shape.
    let s1 = hmFromList [((1, "a"), [0 :: Int]), ((2, "b"), [0])] :: HashMap (Int, String) [Int]
        s2 = hmInsert (3, "c") [0] s1
        s3 = hmInsert (2, "b") [hmSize s2] s1
    assert (show s3 == "{(1,\"a\") -> [0], (2,\"b\") -> [3]}") ("show s3: " <> show s3)
    assert (hmSize s3 == 2) "s3 size"
    -- Both values of one version forcing DIFFERENT versions.
    let f1 = hmFromList [(10, 0 :: Int)] :: HashMap Int Int
        f2 = hmInsert 11 0 f1
        f3 = hmInsert 12 0 f2
        f4 = hmFromList [(20, hmSize f2), (21, hmSize f3), (22, hmSize f1)] :: HashMap Int Int
    assert (show f4 == "{20 -> 2, 21 -> 3, 22 -> 1}") ("show f4: " <> show f4)
    -- A READ whose key reads another version (H1): encoding a structural
    -- key forces its components, and the force reroots the store, so the
    -- reader must encode before it fetches the store. Both directions, the
    -- scalar and fused-case shapes (the key is forced before the call), and
    -- a key component that is itself a lookup in the sibling version.
    let k1 = hmFromList [((1, 1), "a")] :: HashMap (Int, Int) String
        k2 = hmInsert (2, 2) "b" k1
    assert (hmLookup (2, hmSize k2) k1 == Nothing) "structural lookup on the older version"
    assert (not (hmMember (2, hmSize k2) k1)) "structural member on the older version"
    assert (hmLookup (1, hmSize k2 - 1) k1 == Just "a") "structural hit on the older version"
    let d1 = hmFromList [((1, 1), "a"), ((2, 2), "b")] :: HashMap (Int, Int) String
        d2 = hmDelete (2, 2) d1
    assert (hmSize d2 == 1) "d2 size"
    assert (hmLookup (2, hmSize d1) d2 == Nothing) "structural lookup on the newer version"
    assert (not (hmMember (2, hmSize d1) d2)) "structural member on the newer version"
    let p1 = hmFromList [((1, 1), 1)] :: HashMap (Int, Int) Int
        p2 = hmInsert (2, 2) 2 p1
    assert (hmLookup (2, maybe 0 id (hmLookup (2, 2) p2)) p1 == Nothing) "component read by lookup"
    let c1 = hmFromList [(1, "a")] :: HashMap Int String
        c2 = hmInsert 2 "b" c1
    assert (hmLookup (hmSize c2) c1 == Nothing) "scalar lookup on the older version"
    assert (not (hmMember (hmSize c2) c1)) "scalar member on the older version"
    let fused = case hmLookup (hmSize c2) c1 of
            Just v -> "hit " <> v
            Nothing -> "miss"
    assert (fused == "miss") "fused case lookup on the older version"
    let fusedHit = case hmLookup (hmSize c2 - 1) c1 of
            Just v -> "hit " <> v
            Nothing -> "miss"
    assert (fusedHit == "hit a") "fused case hit on the older version"
    let l1 = hmFromList [([1], "a")] :: HashMap [Int] String
        l2 = hmInsert [2] "b" l1
    assert (hmLookup [hmSize l2] l1 == Nothing) "list key lookup on the older version"
    assert (hmLookup [hmSize l2] l2 == Just "b") "list key lookup on the newer version"
    assert (show (hmToList k1) == "[((1,1),\"a\")]" && hmSize k2 == 2) "versions intact"
    -- compare on NaN is GT in every position (GHC: `<`, then `==`, else GT).
    let nan = 0.0 / 0.0 :: Number
    assert (compare nan 1.0 == GT) "compare nan 1.0"
    assert (compare 1.0 nan == GT) "compare 1.0 nan"
    assert (compare nan nan == GT) "compare nan nan"
    assert (compare 1.0 2.0 == LT && compare 2.0 2.0 == EQ && compare 3.0 2.0 == GT) "compare finite"
    putStrLn "enum reroot ok"
