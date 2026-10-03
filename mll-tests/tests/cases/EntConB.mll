module EntConB (lenB, showB, Shape(..), area, Node(..), firstNode) where

data L = Nil | Node Int L
    deriving (Show, Eq)

data Shape = Circle Int | Rect Int Int
    deriving (Show, Eq)

newtype Node = MkNode Int
    deriving (Show, Eq)

build :: [Int] -> L
build [] = Nil
build (x : xs) = Node x (build xs)

count :: L -> Int
count Nil = 0
count (Node _ r) = 1 + count r

lenB :: [Int] -> Int
lenB xs = count (build xs)

showB :: [Int] -> String
showB xs = show (build xs)

area :: Shape -> Int
area (Circle r) = 3 * r * r
area (Rect w h) = w * h

firstNode :: [Int] -> Node
firstNode xs = case build xs of
    Node x _ -> MkNode x
    Nil -> MkNode 0
