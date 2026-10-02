module FixityListed (listedTotal, (<+>), plus) where

infixr 5 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a + b * 3

infixr 5 `plus`
plus :: Int -> Int -> Int
plus a b = a + b * 3

listedTotal :: Int
listedTotal = 1 <+> 2 `plus` 3
