-- Integer arithmetic on values that stay small: the twin uses native Lua
-- numbers, so the ratio prices the always-boxed Integer representation
-- (the Int escape hatch is the arith_loop workload — the gap between the
-- two ratios is what switching a hot loop from Integer to Int buys).
module Main where

-- The accumulator is forced each step (seq), as a Haskell programmer
-- writes any hot reduction — Integer ops are not cheap for the
-- cheapness analysis, so a lazy accumulator would be a 200000-deep
-- thunk chain.
go :: Integer -> Integer -> Integer -> Integer
go n acc i
    | i > n     = acc
    | otherwise = let acc' = (acc * 7 + i) `mod` 1000000007
                  in acc' `seq` go n acc' (i + 1)

main :: IO ()
main = print (go 200000 0 1)
