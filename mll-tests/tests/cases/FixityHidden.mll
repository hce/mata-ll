module FixityHidden where

infixr 5 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a + b * 4

infixr 5 `plus`
plus :: Int -> Int -> Int
plus a b = a + b * 4

hiddenTotal :: Int
hiddenTotal = 1 <+> 2 `plus` 3
