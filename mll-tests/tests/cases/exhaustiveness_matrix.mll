-- Exhaustiveness under the matrix checker (F4): complete Bool matches,
-- component-checked tuples, nested constructor arguments, later argument
-- columns, and the deliberately PERMISSIVE shapes (literal matches, guard
-- fall-offs handled at runtime) — all must compile and match GHC.
module Main where

data C = R | G | B

full :: Bool -> Int
full True = 1
full False = 0

pair :: (Bool, C) -> Int
pair (True, _) = 1
pair (False, R) = 2
pair (False, G) = 3
pair (False, B) = 4

nested :: Maybe Bool -> Int
nested Nothing = 0
nested (Just True) = 1
nested (Just False) = 2

second :: Int -> C -> Int
second n R = n
second n G = n + 1
second n B = n + 2

lits :: Int -> Int
lits 0 = 1
lits 1 = 2
lits n = n

go :: Int -> [Int] -> Int
go acc (0 : rest) = go acc rest
go acc (b : rest) = go (acc + b) rest
go acc [] = acc

main :: IO ()
main = do
  print (full False)
  print (pair (False, B))
  print (nested (Just False))
  print (second 1 B)
  print (lits 7)
  print (go 0 [1, 0, 2, 3])

-- expect: 0
-- expect: 4
-- expect: 2
-- expect: 3
-- expect: 7
-- expect: 6
