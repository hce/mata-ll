-- The Prelude's association-list `lookup` (it was missing; only
-- Data.Map's alias-qualified `lookup` existed).
main :: IO ()
main = do
    print (lookup 2 [(1, "one"), (2, "two"), (2, "deux")])
    print (lookup 9 [(1, "one")])
    print (lookup "b" ([] :: [(String, Int)]))
    print (lookup True [(False, 0), (True, 1)])
