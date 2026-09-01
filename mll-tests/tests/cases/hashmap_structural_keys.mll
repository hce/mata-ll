-- A17: structural HashMap keys — tuples, lists, Maybe (nested too). The
-- scalar path keys the Lua table directly (unchanged); a structural key
-- goes through the encoded-entry variants: an injective string encoding
-- keys the table, {key, value} entries keep the original for iteration,
-- and hmKeys/hmValues/hmToList sort by the A16 structural compare, so
-- iteration is true Ord order. Hashable is structural like Show/Eq/Ord.

main :: IO ()
main = do
    -- the motivating shape: a coordinate grid
    let grid = hmFromList [((0, 1), "a"), ((2, 0), "b"), ((0, 0), "c")]
    assert (hmLookup (0, 1) grid == Just "a") "tuple lookup hit"
    assert (hmLookup (1, 1) grid == Nothing) "tuple lookup miss"
    assert (hmMember (2, 0) grid) "tuple member"
    assert (hmSize grid == 3) "tuple size"
    print (hmKeys grid)
    print (hmToList (hmDelete (0, 0) grid))
    -- inserting an existing key REPLACES it (value semantics, the exact
    -- thing identity-keyed tables got wrong)
    let grid2 = hmInsert (0, 1) "z" grid
    assert (hmSize grid2 == 3) "insert replaces, size stays"
    assert (hmLookup (0, 1) grid2 == Just "z") "insert replaces value"
    -- list and Maybe keys, and a nested composite
    let m2 = hmInsert [1, 2] True (hmInsert [1] False hmEmpty)
    assert (hmLookup [1, 2] m2 == Just True) "list key"
    print (hmKeys m2)
    let m3 = hmInsert (Just (1, "x")) 9 hmEmpty
    assert (hmLookup (Just (1, "x")) m3 == Just 9) "nested Maybe-tuple key"
    assert (hmLookup Nothing (m3 :: HashMap (Maybe (Int, String)) Int) == Nothing) "Nothing key miss"
    -- string keys inside composites cannot collide with the separators
    let s = hmInsert ("a,b", "c") 1 (hmInsert ("a", "b,c") 2 hmEmpty)
    assert (hmSize s == 2) "string encoding is injective"
    assert (hmLookup ("a", "b,c") s == Just 2) "comma string key"
    putStrLn "ok"

-- expect: [(0,0),(0,1),(2,0)]
-- expect: [((0,1),"a"),((2,0),"b")]
-- expect: [[1],[1,2]]
-- expect: ok
