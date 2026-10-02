module FixityAmbRight where

infixl 6 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a + b
