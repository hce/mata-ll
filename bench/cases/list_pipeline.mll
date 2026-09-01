-- Lazy list pipeline: range, map, filter, strict fold. The twin fuses
-- the whole pipeline into one loop with no intermediate structure, which
-- is what a Lua programmer writes — the ratio prices cons cells, thunks
-- and per-element closure calls.
module Main where

import Data.List (foldl')

main :: IO ()
main = print (foldl' step 0 (filter odd (map (* 3) [1 .. 200000 :: Int])))
  where
    step a x = (a + x) `mod` 1000000007
