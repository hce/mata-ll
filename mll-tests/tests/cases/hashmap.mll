-- HashMap tests: O(1) dictionary backed by Lua tables.

fromMaybe :: a -> Maybe a -> a
fromMaybe def Nothing = def
fromMaybe _ (Just x) = x

pairsEq :: [(String, Int)] -> [(String, Int)] -> Bool
pairsEq [] [] = True
pairsEq ((k1, v1) : rest1) ((k2, v2) : rest2) = k1 == k2 && v1 == v2 && pairsEq rest1 rest2
pairsEq _ _ = False

main :: IO ()
main = do
    let m = hmInsert "alice" 30 $ hmInsert "bob" 25 $ hmInsert "charlie" 35 $ hmEmpty
    assert (hmSize m == 3) "size 3"
    assert (fromMaybe 0 (hmLookup "bob" m) == 25) "lookup bob"
    assert (fromMaybe 0 (hmLookup "dave" m) == 0) "lookup missing"
    assert (hmMember "alice" m) "member alice"
    assert (not (hmMember "dave" m)) "not member dave"
    assert (length (hmKeys m) == 3) "keys length"
    assert (length (hmValues m) == 3) "values length"
    -- Delete
    let m2 = hmDelete "bob" m
    assert (hmSize m2 == 2) "size after delete"
    assert (fromMaybe 0 (hmLookup "bob" m2) == 0) "deleted key gone"
    assert (fromMaybe 0 (hmLookup "alice" m2) == 30) "other keys intact"
    -- Update
    let m3 = hmInsert "alice" 31 m
    assert (fromMaybe 0 (hmLookup "alice" m3) == 31) "update"
    assert (hmSize m3 == 3) "size after update"
    -- fromList
    let m4 = hmFromList [("x", 1), ("y", 2), ("x", 3)]
    assert (hmSize m4 == 2) "fromList dedups keys"
    assert (fromMaybe 0 (hmLookup "x" m4) == 3) "fromList last write wins"
    assert (fromMaybe 0 (hmLookup "y" m4) == 2) "fromList other key"
    assert (not (hmMember "z" m4)) "fromList missing key"
    assert (hmSize (hmFromList []) == 0) "fromList empty"
    -- toList
    assert (length (hmToList m) == 3) "toList length"
    assert (pairsEq (hmToList m) (zip (hmKeys m) (hmValues m))) "toList matches zip keys values"
    assert (pairsEq (hmToList m) [("alice", 30), ("bob", 25), ("charlie", 35)]) "toList sorted by key"
    assert (pairsEq (hmToList (hmFromList (hmToList m))) (hmToList m)) "toList/fromList round-trip"
    assert (length (hmToList hmEmpty) == 0) "toList empty"
