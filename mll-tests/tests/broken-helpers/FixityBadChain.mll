module FixityBadChain where

infix 4 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a - b

chained :: Int
chained = 1 <+> 2 <+> 3
