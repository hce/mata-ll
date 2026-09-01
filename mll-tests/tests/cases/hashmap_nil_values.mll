-- A nil-represented VALUE (Nothing, [], ()) under a scalar key: `t[k] = v`
-- with v == nil is Lua's delete, so the raw store silently dropped the
-- entry — hmFromList [(7, Nothing)] had size 0, lookup missed, iteration
-- skipped. Found by the grown backend fuzzer (batch index 7 of its first
-- run). The scalar path now boxes nil values behind a unique sentinel
-- (__mll_hm_nilv, compared with rawequal — a plain == would fire the
-- Integer __eq metamethod on limb-table values) and unwraps on every
-- read; the encoded structural-key path stores {k, v} entry tables and
-- never had the problem (its half is pinned here as a control).

main :: IO ()
main = do
    -- Nothing as a value, scalar Int key
    let m1 = hmFromList ([(7, Nothing)] :: [(Int, Maybe Int)])
    assert (hmSize m1 == 1) "Nothing value keeps its entry"
    assert (hmMember 7 m1) "member sees the entry"
    print (hmLookup 7 m1)
    print (hmToList m1)
    -- [] as a value, scalar Bool key; hmValues must carry it through
    let m2 = hmFromList ([(True, [])] :: [(Bool, [Int])])
    assert (hmSize m2 == 1) "empty-list value keeps its entry"
    print (hmValues m2)
    print (elem ([] :: [Int]) (hmValues m2))
    -- insert (not just fromList) takes the same path
    let m3 = hmInsert 1 Nothing (hmFromList ([(2, Just 5)] :: [(Int, Maybe Int)]))
    assert (hmSize m3 == 2) "inserted Nothing keeps its entry"
    print (hmToList m3)
    -- overwriting a nil-represented value with a real one and back
    let m4 = hmInsert 1 (Just 9) m3
    print (hmLookup 1 m4)
    print (hmLookup 1 (hmInsert 1 Nothing m4))
    -- deleting the entry really deletes it (the sentinel is not sticky)
    assert (hmSize (hmDelete 1 m3) == 1) "delete removes a boxed entry"
    -- non-nil values are untouched by the boxing, Integer limb tables
    -- included (the rawequal lesson)
    let m5 = hmFromList ([(False, 393410916044004406428136), (True, (-4))] :: [(Bool, Integer)])
    print (hmToList m5)
    -- control: the encoded structural-key path stores entries either way
    let s1 = hmFromList ([([1], Nothing)] :: [([Int], Maybe Int)])
    assert (hmSize s1 == 1) "structural key, Nothing value"
    print (hmLookup [1] s1)
    print (hmToList s1)
    putStrLn "ok"

-- expect: Just Nothing
-- expect: [(7,Nothing)]
-- expect: [[]]
-- expect: True
-- expect: [(1,Nothing),(2,Just 5)]
-- expect: Just (Just 9)
-- expect: Just Nothing
-- expect: [(False,393410916044004406428136),(True,-4)]
-- expect: Just Nothing
-- expect: [([1],Nothing)]
-- expect: ok
