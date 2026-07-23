-- ghc_regr005: HashMap operations: insert, lookup, delete, fold

fromMaybe :: a -> Maybe a -> a
fromMaybe d Nothing  = d
fromMaybe _ (Just x) = x

main :: IO ()
main = do
    -- Build a map from scratch
    let m0 = hmEmpty :: HashMap String Int
    let m1 = hmInsert "a" 1 m0
    let m2 = hmInsert "b" 2 m1
    let m3 = hmInsert "c" 3 m2
    let m4 = hmInsert "d" 4 m3

    -- Size
    assert (hmSize m4 == 4) "size 4"
    assert (hmSize m0 == 0) "size 0"

    -- Lookup hits
    assert (fromMaybe 0 (hmLookup "a" m4) == 1) "lookup a"
    assert (fromMaybe 0 (hmLookup "b" m4) == 2) "lookup b"
    assert (fromMaybe 0 (hmLookup "c" m4) == 3) "lookup c"
    assert (fromMaybe 0 (hmLookup "d" m4) == 4) "lookup d"

    -- Lookup miss
    assert (hmLookup "z" m4 == Nothing) "lookup miss"

    -- Member
    assert (hmMember "a" m4 == True) "member a"
    assert (hmMember "z" m4 == False) "member z"

    -- Delete
    let m5 = hmDelete "b" m4
    assert (hmSize m5 == 3) "size after delete"
    assert (hmLookup "b" m5 == Nothing) "deleted gone"
    assert (fromMaybe 0 (hmLookup "a" m5) == 1) "a still there"
    assert (fromMaybe 0 (hmLookup "c" m5) == 3) "c still there"

    -- Overwrite
    let m6 = hmInsert "a" 99 m4
    assert (fromMaybe 0 (hmLookup "a" m6) == 99) "overwrite a"
    assert (hmSize m6 == 4) "size unchanged after overwrite"

    -- Fold: sum all values via hmValues
    let total = foldl (\acc v -> acc + v) 0 (hmValues m4)
    assert (total == 10) "foldl sum"

    -- Keys and values length
    assert (length (hmKeys m4) == 4) "keys length"
    assert (length (hmValues m4) == 4) "values length"

    putStrLn "ok"
