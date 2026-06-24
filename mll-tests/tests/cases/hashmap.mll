-- HashMap tests: O(1) dictionary backed by Lua tables.

fromMaybe :: a -> Maybe a -> a
fromMaybe def Nothing = def
fromMaybe _ (Just x) = x

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
