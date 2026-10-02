module FixityQual (qualTotal, (<+>), plus) where

infixr 5 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a + b * 2

infixr 5 `plus`
plus :: Int -> Int -> Int
plus a b = a + b * 2

qualTotal :: Int
qualTotal = 1 <+> 2 `plus` 3
