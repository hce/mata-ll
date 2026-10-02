module ScopeTake where

import Prelude hiding (takeWhile)

takeWhile :: (a -> Bool) -> [a] -> [a]
takeWhile _ [] = []
takeWhile p (x : xs) = if p x then x : takeWhile p xs else [x]

upTo :: Int -> [Int] -> [Int]
upTo n xs = takeWhile (\x -> x < n) xs

firstOf :: [Int] -> Int
firstOf xs = head xs
