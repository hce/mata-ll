-- HashMap churn — and this time the name is earned: every iteration
-- inserts one key, deletes one key, and looks one up, over a map held
-- near 500 entries. hmInsert/hmDelete are persistent (full copy per
-- operation), the twin mutates one table in place — the ratio prices
-- value semantics on WRITE turnover, with just enough lookup traffic
-- to keep the result honest. The read path has its own benchmark now
-- (hm_lookup); the original hm_churn was 81% lookups by wall time.
module Main where

churn :: Int -> Int -> HashMap Int Int -> Int
churn 0 acc m = (acc + hmSize m) `mod` 1000000007
churn i acc m =
    let m1 = hmInsert (i `mod` 500) (i * i) m
        m2 = hmDelete ((i * 7) `mod` 500) m1
    in case hmLookup ((i * 3) `mod` 500) m2 of
        Just v  -> churn (i - 1) ((acc + v) `mod` 1000000007) m2
        Nothing -> churn (i - 1) acc m2

main :: IO ()
main = print (churn 30000 0 hmEmpty)
