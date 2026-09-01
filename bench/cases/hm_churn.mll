-- HashMap churn: build a 2000-key map by sequential insert, hammer it
-- with 1000000 lookups, then delete half the keys and count survivors.
-- hmInsert/hmDelete are persistent (full copy per operation), the twin
-- mutates one table in place — the ratio prices value semantics.
module Main where

build :: Int -> HashMap Int Int -> HashMap Int Int
build 0 m = m
build i m = build (i - 1) (hmInsert i (i * i) m)

lookups :: Int -> Int -> HashMap Int Int -> Int
lookups 0 acc _ = acc
lookups i acc m = case hmLookup (i `mod` 2000 + 1) m of
    Just v  -> lookups (i - 1) ((acc + v) `mod` 1000000007) m
    Nothing -> lookups (i - 1) acc m

deleteHalf :: Int -> HashMap Int Int -> HashMap Int Int
deleteHalf 0 m = m
deleteHalf i m = deleteHalf (i - 1) (hmDelete (i * 2) m)

main :: IO ()
main = do
    let m = build 2000 hmEmpty
    print (lookups 1000000 0 m)
    print (hmSize (deleteHalf 1000 m))
