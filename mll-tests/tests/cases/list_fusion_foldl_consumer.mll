-- Pins the list-pipeline fusion's foldl' consumer on the shapes whose
-- fused loop was lost while the recognizer matched a stale spelling of
-- the list instance's method name (Q44): the benchmark pipeline (a
-- where-bound step over filter/map/range), an operator fold over a map,
-- a lambda fold over a filter, a leaf source, and a fold over a Maybe,
-- which is not a list pipeline at all. The values are GHC's whatever
-- the emission shape; the shape itself is pinned by the codegen test
-- foldl_consumer_fuses_through_the_method_origin.

module Main where

import Data.List (foldl')

pipeline :: Int -> Int
pipeline n = foldl' step 0 (filter odd (map (* 3) [1 .. n]))
  where
    step a x = (a + x) `mod` 1000000007

leaf :: [Int] -> Int
leaf xs = foldl' (\acc x -> acc + x * x) 0 (filter even xs)

main :: IO ()
main = do
    print (pipeline 200000)
    print (foldl' (+) 0 (map (* 3) [1 .. 20 :: Int]))
    print (foldl' (\acc x -> acc + x * x) 0 (filter even [1 .. 20 :: Int]))
    print (leaf [1 .. 20])
    print (foldl' (+) 0 (Just (5 :: Int)))
