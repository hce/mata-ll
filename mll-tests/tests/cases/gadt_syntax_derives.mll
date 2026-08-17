-- Test: derived Eq/Ord/Show/Enum/Bounded/Functor on constructors written in
-- GADT SYNTAX read their arity from the registered constructor info, not
-- from the parser's field list (which is empty for `Con :: a -> b -> T`).
-- The derives used to see zero fields, so `Pt 1 2 == Pt 3 4` was True,
-- `show (Pt 1 2)` was "Pt", and Ord compared tags only.

data Pt where
    Pt :: Int -> Int -> Pt
    deriving (Eq, Show, Ord)

data Shape where
    Circle :: Int -> Shape
    Rect :: Int -> Int -> Shape
    deriving (Eq, Show, Ord)

data Color where
    Red :: Color
    Green :: Color
    Blue :: Color
    deriving (Eq, Show, Ord, Enum, Bounded)

data Box a where
    Box :: a -> Box a
    deriving (Eq, Show, Functor)

main :: IO ()
main = do
    assert (Pt 1 2 == Pt 1 2) "Eq: equal fields"
    assert (not (Pt 1 2 == Pt 3 4)) "Eq: differing fields are not equal"
    assert (show (Pt 1 2) == "Pt 1 2") "Show prints the fields"
    assert (Pt 1 2 < Pt 1 3) "Ord compares fields, not only tags"
    assert (show (Rect 2 3) == "Rect 2 3") "Show on the second constructor"
    assert (Circle 5 < Rect 0 0) "Ord: constructor order first"
    assert (not (Rect 2 3 == Rect 2 4)) "Eq on a two-constructor type"
    assert ([minBound .. maxBound] == [Red, Green, Blue]) "Enum/Bounded on GADT-syntax enum"
    assert (fromEnum Blue == 2 && toEnum 1 == Green) "fromEnum/toEnum"
    assert (fmap (+ 1) (Box 41) == Box 42) "Functor over the GADT-syntax field"
    assert (show (Box 7) == "Box 7") "Show on a parameterised GADT-syntax type"
