-- HashMap lookup hammer: build a 2000-key map once, then hit it with
-- 1000000 hmLookups against a mutating accumulator. The map never
-- changes after the build, so this prices exactly the read path — the
-- lookup call, the Maybe in the result, the case dispatch. (The old
-- hm_churn bundled this with the build and delete phases; measured,
-- the lookups were 81% of its wall time, so the turnover cost hid
-- inside a mislabeled number. See hm_churn for the actual churn.)
module Main where

build :: Int -> HashMap Int Int -> HashMap Int Int
build 0 m = m
build i m = build (i - 1) (hmInsert i (i * i) m)

lookups :: Int -> Int -> HashMap Int Int -> Int
lookups 0 acc _ = acc
lookups i acc m = case hmLookup (i `mod` 2000 + 1) m of
    Just v  -> lookups (i - 1) ((acc + v) `mod` 1000000007) m
    Nothing -> lookups (i - 1) acc m

main :: IO ()
main = print (lookups 1000000 0 (build 2000 hmEmpty))
