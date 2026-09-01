-- Control workload: tail-recursive Int arithmetic, no data structures.
-- The ratio here bounds pure call/arithmetic overhead — every other
-- workload's ratio includes at least this much.
module Main where

go :: Int -> Int -> Int -> Int
go n acc i
    | i > n     = acc
    | otherwise = go n ((acc + i) `mod` 1000000007) (i + 1)

main :: IO ()
main = print (go 5000000 0 1)
