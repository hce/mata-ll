-- Demand-strict guard params: a guard chain that forces a parameter on
-- EVERY path (the first condition always runs; every body's path forces
-- the rest) gets one entry force instead of a force per use. The other
-- direction is the contract that matters: a parameter demanded only on
-- SOME guard path must stay lazy — bottom there is observable only when
-- that path actually runs.

count :: Int -> Int -> Int
count n i | i >= 10 = n
          | otherwise = count (n + 1) (i + 1)

pick :: Int -> Int -> Int
pick x y | x > 0 = x
         | otherwise = y

main :: IO ()
main = do
    assert (count 0 0 == 10) "strict guard params entry-forced"
    assert (pick 1 (error "must stay unforced") == 1) "partially-demanded param stays lazy"
