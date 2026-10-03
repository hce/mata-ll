import EntConA
import qualified EntConB as B

data Leaf = Leaf String | Node Int
    deriving (Show, Eq)

data Pair = Pair Shape B.Shape
    deriving Show

describe :: Shape -> String
describe (Circle r) = "A circle " <> show r
describe (Square s) = "A square " <> show s

describeB :: B.Shape -> String
describeB (B.Circle r) = "B circle " <> show r
describeB (B.Rect w h) = "B rect " <> show (w * h)

leafName :: Leaf -> String
leafName (Leaf s) = s
leafName (Node n) = "node " <> show n

main :: IO ()
main = do
    print (sumA [1, 2, 3], B.lenB [1, 2, 3], depthA [4, 5])
    putStrLn (showA [7])
    putStrLn (B.showB [7, 8])
    print (Leaf "mine", Node 3)
    putStrLn (leafName (Leaf "x") <> " " <> leafName (Node 4))
    print (Pair (Circle 1) (B.Circle 2))
    putStrLn (describe (Square 2) <> "; " <> describeB (B.Rect 2 3))
    print (B.area (B.Circle 2), B.area (B.Rect 4 5))
    print (Circle 1 < Square 0, Circle 2 == Circle 2, B.Circle 1 == B.Rect 1 1)
    print (B.firstNode [9, 8], B.MkNode 3 == B.firstNode [3])
    print allTags
    print (Red, [Red ..], succ Red)
    print (map describe [Circle 3, Square 4])
