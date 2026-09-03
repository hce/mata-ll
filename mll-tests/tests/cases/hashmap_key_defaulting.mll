-- An unannotated numeric literal as a HashMap key defaults to Int (the
-- mata-ll rule: GHC's Data.Map twin would default to Integer, which has no
-- Hashable instance here). The key type used to stay UNRESOLVED — the
-- variable counted as "determined" because a let-binder mentioned it — so
-- the literal compiled to a raw Lua number and `show` fell to the
-- type-erased runtime show, printing `()` for the whole map.
-- HashMap builtins are outside the GHC oracle: self-asserting.

main :: IO ()
main = do
    let d1 = hmFromList [(1, "x"), (2, "y")]
        k = 3 - 2
    assert (show d1 == "{1 -> \"x\", 2 -> \"y\"}") ("show d1: " <> show d1)
    assert (hmLookup k d1 == Just "x") "computed Int key"
    assert (hmLookup (k + 1) d1 == Just "y") "computed Int key 2"
    let s = hmFromList [((1, "a"), 5), ((2, "b"), 6)]
    assert (show s == "{(1,\"a\") -> 5, (2,\"b\") -> 6}") ("show s: " <> show s)
    assert (hmLookup (1, "a") s == Just 5) "structural key with defaulted component"
    let m = hmInsert 10 "ten" hmEmpty
    assert (show m == "{10 -> \"ten\"}") "hmInsert literal key"
    putStrLn "key defaulting ok"
