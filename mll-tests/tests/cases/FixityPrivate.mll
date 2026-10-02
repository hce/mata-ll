module FixityPrivate (privateTotal) where

infixr 5 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a + b * 2

privateTotal :: Int
privateTotal = 1 <+> 2 <+> 3
