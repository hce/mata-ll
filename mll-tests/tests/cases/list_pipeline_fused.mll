-- The list-pipeline fusion (fuse.rs): foldl' over map/filter chains and
-- ranges emits one loop with no intermediate lists. Pins the fused
-- shapes byte-exact against GHC: range and leaf sources, stage nesting
-- on both sides of a filter (a map outside a filter must see survivors
-- only), the empty range and empty leaf, a seed other than zero, and a
-- fold function -- (-) resolves to a subtraction primitive -- whose
-- argument order the loop must preserve.

module Main where

import Data.List (foldl')

step :: Int -> Int -> Int
step a x = (a + x) `mod` 97

sq :: Int -> Int
sq x = x * x

small :: Int -> Bool
small x = x < 40

addTo :: Int -> [Int] -> Int
addTo z xs = foldl' step z (map sq (filter small xs))

main :: IO ()
main = do
    print (foldl' step 0 (filter odd (map (* 3) [1 .. 50])))
    print (foldl' step 7 (map sq [1 .. 10]))
    print (foldl' step 0 (filter even [1 .. 20]))
    print (addTo 5 [1, 39, 40, 41, 2])
    print (foldl' step 0 (map sq (filter small (map (+ 1) [35 .. 45]))))
    print (foldl' step 3 (filter odd (map sq ([] :: [Int]))))
    print (foldl' (-) 100 (map (* 2) [1 .. 4]))

-- expect: 32
-- expect: 4
-- expect: 13
-- expect: 76
-- expect: 4
-- expect: 3
-- expect: 80
