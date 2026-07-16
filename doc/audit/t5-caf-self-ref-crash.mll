module Main where
data Stream = S Integer Stream
sHead :: Stream -> Integer
sHead (S x _) = x
sTail :: Stream -> Stream
sTail (S _ r) = r
s :: Stream
s = S 1 s
main :: IO ()
main = print (sHead (sTail s))
