-- Stress test: large pattern matching with many branches and nested patterns

data Shape = Circle Integer | Rect Integer Integer | Triangle Integer Integer Integer | Point
    deriving (Show, Eq)

data Color = Red | Green | Blue | Yellow | White | Black
    deriving (Show, Eq)

data Colored = Colored Color Shape
    deriving (Show, Eq)

area :: Shape -> Integer
area (Circle r) = r * r * 3
area (Rect w h) = w * h
area (Triangle a b c) = a + b + c
area Point = 0

classify :: Colored -> String
classify (Colored c s) = classifyHelper c s

classifyHelper :: Color -> Shape -> String
classifyHelper c (Circle r) =
    if c == Red then (if r > 10 then "big red circle" else "small red circle")
    else if c == Green then "green circle"
    else if c == Blue then "blue circle"
    else "other circle"
classifyHelper c (Rect w h) =
    if c == Red then (if w == h then "red square" else "red rect")
    else if c == Green then "green rect"
    else if c == Blue then "blue rect"
    else if c == Yellow then "yellow rect"
    else if c == White then "white rect"
    else if c == Black then "black rect"
    else "rect"
classifyHelper _ (Triangle _ _ _) = "triangle"
classifyHelper _ Point = "point"

colorValue :: Color -> Integer
colorValue Red = 1
colorValue Green = 2
colorValue Blue = 3
colorValue Yellow = 4
colorValue White = 5
colorValue Black = 6

data Container = Box Colored | Empty
    deriving (Show, Eq)

containerArea :: Container -> Integer
containerArea Empty = 0
containerArea (Box (Colored _ s)) = area s

main :: IO ()
main = do
    assert (area (Circle 5) == 75) "circle area"
    assert (area (Rect 3 4) == 12) "rect area"
    assert (area Point == 0) "point area"
    assert (classify (Colored Red (Circle 20)) == "big red circle") "big red circle"
    assert (classify (Colored Red (Circle 5)) == "small red circle") "small red circle"
    assert (classify (Colored Green (Circle 1)) == "green circle") "green circle"
    assert (classify (Colored Red (Rect 5 5)) == "red square") "red square"
    assert (classify (Colored Red (Rect 3 5)) == "red rect") "red rect"
    assert (classify (Colored Yellow (Rect 1 2)) == "yellow rect") "yellow rect"
    assert (classify (Colored Black (Triangle 3 4 5)) == "triangle") "triangle"
    assert (classify (Colored Red Point) == "point") "point"
    assert (classify (Colored Yellow (Circle 1)) == "other circle") "other circle"
    assert (colorValue Red == 1) "red val"
    assert (colorValue Black == 6) "black val"
    assert (containerArea Empty == 0) "empty container"
    assert (containerArea (Box (Colored Red (Circle 4))) == 48) "boxed circle"
    putStrLn "ok"
