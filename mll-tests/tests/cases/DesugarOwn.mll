module DesugarOwn (concatMap, enumFromTo, error, (&&), pairsUp, upTo, firstOf, band, total) where

import Prelude hiding (concatMap, enumFromTo, error, (&&))

concatMap :: (a -> [b]) -> [a] -> [b]
concatMap f xs = reverse (go xs)
  where
    go [] = []
    go (y : ys) = f y ++ go ys

enumFromTo :: Int -> Int -> [Int]
enumFromTo a b = [b, a]

error :: String -> Int
error msg = 0 - 1

(&&) :: Int -> Int -> Int
(&&) a b = a * 10 + b

pairsUp :: [Int] -> [Int]
pairsUp xs = [x * 10 | x <- xs]

upTo :: Int -> [Int]
upTo n = [1 .. n]

firstOf :: [Int] -> Int
firstOf xs = (\(x : _) -> x) xs

band :: Int -> String
band n
    | n > 0, n < 10 = "digit"
    | n >= 10, even n = "even"
    | otherwise = "other"

total :: [Maybe Int] -> IO Int
total ms = do
    [Just a, Just b] <- pure ms
    pure (a + b)
