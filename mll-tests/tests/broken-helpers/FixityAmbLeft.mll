module FixityAmbLeft where

infix 4 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a - b
