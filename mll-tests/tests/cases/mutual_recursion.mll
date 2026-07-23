-- Mutually recursive data types and functions

data Forest = Empty | Cons Tree Forest
data Tree = Node Int Forest

countNodes :: Tree -> Int
countNodes (Node _ forest) = 1 + countForest forest

countForest :: Forest -> Int
countForest Empty = 0
countForest (Cons t f) = countNodes t + countForest f

main :: IO ()
main = do
    assert (countNodes (Node 1 Empty) == 1) "single node"
    assert (countForest Empty == 0) "empty forest"
    let f = Cons (Node 42 (Cons (Node 23 Empty) Empty)) (Cons (Node 5 Empty) Empty)
    assert (countForest f == 3) "forest with 3 nodes"
