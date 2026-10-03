module EntConA (Shape(..), sumA, depthA, showA, Tag(Red, Green), allTags) where

data T = Leaf | Node T Int T
    deriving (Show, Eq)

data Shape = Circle Int | Square Int
    deriving (Show, Eq, Ord)

data Tag = Red | Green | Blue
    deriving (Show, Eq, Ord, Enum, Bounded)

build :: [Int] -> T
build [] = Leaf
build (x : xs) = Node Leaf x (build xs)

total :: T -> Int
total Leaf = 0
total (Node l x r) = total l + x + total r

depth :: T -> Int
depth Leaf = 0
depth (Node l _ r) = 1 + max (depth l) (depth r)

sumA :: [Int] -> Int
sumA xs = total (build xs)

depthA :: [Int] -> Int
depthA xs = depth (build xs)

showA :: [Int] -> String
showA xs = show (build xs)

allTags :: [Tag]
allTags = [minBound .. maxBound]
