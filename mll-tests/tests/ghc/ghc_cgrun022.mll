-- GHC cgrun022: Typeclass instances and deriving
-- Tests Show, Eq, Ord on custom types

data Color = Red | Green | Blue
    deriving (Show, Eq, Ord)

data Shape = Circle Int | Rect Int Int
    deriving (Show, Eq)

area :: Shape -> Int
area (Circle r) = r * r * 3
area (Rect w h) = w * h

main :: IO ()
main = do
    -- Show
    assert (show Red == "Red") "show Red"
    assert (show Green == "Green") "show Green"
    assert (show (Circle 5) == "Circle 5") "show Circle"
    assert (show (Rect 2 3) == "Rect 2 3") "show Rect"

    -- Eq
    assert (Red == Red) "Red == Red"
    assert (Red /= Blue) "Red /= Blue"
    assert (Circle 3 == Circle 3) "Circle eq"
    assert (Circle 3 /= Circle 4) "Circle neq"
    assert (Rect 2 3 == Rect 2 3) "Rect eq"
    assert (Circle 1 /= Rect 1 1) "Circle /= Rect"

    -- Ord
    assert (Red < Green) "Red < Green"
    assert (Green < Blue) "Green < Blue"
    assert (Blue > Red) "Blue > Red"
    assert (min Red Blue == Red) "min"
    assert (max Red Blue == Blue) "max"

    putStrLn "ok"
