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
    -- compare on NaN is GT in every position (GHC: `<`, then `==`, else GT).
    let nan = 0.0 / 0.0 :: Number
    assert (compare nan 1.0 == GT) "compare nan 1.0"
    assert (compare 1.0 nan == GT) "compare 1.0 nan"
    assert (compare nan nan == GT) "compare nan nan"
    assert (compare 1.0 2.0 == LT && compare 2.0 2.0 == EQ && compare 3.0 2.0 == GT) "compare finite"
    putStrLn "enum reroot ok"
