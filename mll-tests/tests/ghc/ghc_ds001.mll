-- GHC ds001: Pattern matching exhaustiveness
-- Tests that all patterns are handled correctly

data Color = Red | Green | Blue
    deriving (Show, Eq)

data Shape = Circle Number | Square Number | Triangle Number Number Number
    deriving (Show, Eq)

colorToInt :: Color -> Integer
colorToInt Red   = 0
colorToInt Green = 1
colorToInt Blue  = 2

perimeter :: Shape -> Number
perimeter (Circle r)       = 2.0 * 3.14159 * r
perimeter (Square s)       = 4.0 * s
perimeter (Triangle a b c) = a + b + c

-- Nested pattern matching
data Pair a b = MkPair a b

firstColor :: Pair Color Integer -> Color
firstColor (MkPair c _) = c

-- Wildcard in the middle
middle :: Integer -> Integer -> Integer -> Integer
middle _ x _ = x

main :: IO ()
main = do
    assert (colorToInt Red == 0) "red"
    assert (colorToInt Green == 1) "green"
    assert (colorToInt Blue == 2) "blue"

    let c = Circle 5.0
    let s = Square 3.0
    let t = Triangle 3.0 4.0 5.0
    assert (perimeter s == 12.0) "square perim"
    assert (perimeter t == 12.0) "triangle perim"

    assert (firstColor (MkPair Red 42) == Red) "nested pair"
    assert (middle 1 2 3 == 2) "wildcard middle"

    putStrLn "ok"
