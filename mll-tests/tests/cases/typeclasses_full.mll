-- Comprehensive typeclass tests

-- Custom typeclass
data Shape = Circle Number | Rect Number Number
    deriving (Show, Eq)

data Color = Red | Green | Blue
    deriving (Show, Eq, Ord)

data Priority = Low | Medium | High
    deriving (Show, Eq, Ord)

-- Deriving Show
data Pair a = MkPair a a
    deriving Show

-- Deriving Eq with fields
data Point = Point Integer Integer
    deriving (Show, Eq)

-- Eq for parameterized types
data Box a = Box a
    deriving (Show, Eq)

-- Ord ordering (declaration order)
data Suit = Clubs | Diamonds | Hearts | Spades
    deriving (Show, Eq, Ord)

main :: IO ()
main = do
    -- Show deriving
    assert (show Red == "Red") "show enum"
    assert (show (Circle 3.0) == "Circle 3") "show con fields"
    assert (show (Rect 2.0 3.0) == "Rect 2 3") "show two fields"
    assert (show (MkPair 1 2) == "MkPair 1 2") "show param type"

    -- Eq deriving - enums
    assert (Red == Red) "eq same"
    assert (Red /= Blue) "neq diff"
    assert (Green == Green) "eq green"

    -- Eq deriving - with fields
    assert (Point 1 2 == Point 1 2) "eq point same"
    assert (Point 1 2 /= Point 1 3) "neq point diff"
    assert (Circle 3.0 == Circle 3.0) "eq circle"
    assert (Circle 3.0 /= Rect 3.0 3.0) "neq diff constructors"

    -- Ord deriving - enums
    assert (Red < Green) "ord lt"
    assert (Green < Blue) "ord lt 2"
    assert (Blue > Red) "ord gt"
    assert (Red <= Red) "ord le eq"
    assert (Red <= Green) "ord le lt"
    assert (Blue >= Green) "ord ge"

    -- Ord declaration order
    assert (Clubs < Diamonds) "suit order 1"
    assert (Diamonds < Hearts) "suit order 2"
    assert (Hearts < Spades) "suit order 3"
    assert (Low < Medium) "priority order"
    assert (Medium < High) "priority order 2"

    -- Eq for lists
    assert ([1, 2, 3] == [1, 2, 3]) "list eq"
    assert ([1, 2] /= [1, 2, 3]) "list neq length"
    assert ([1, 2, 3] /= [1, 2, 4]) "list neq elem"
    assert (([] :: [Integer]) == []) "list eq empty"

    -- Eq for Maybe
    assert (Just 42 == Just 42) "maybe eq just"
    assert (Nothing == (Nothing :: Maybe Integer)) "maybe eq nothing"
    assert (Just 1 /= Just 2) "maybe neq"
    assert (Just 1 /= Nothing) "maybe just vs nothing"

    -- Eq for nested types
    assert ([Just 1, Nothing] == [Just 1, Nothing]) "list of maybe eq"
    assert ([Just 1, Nothing] /= [Just 2, Nothing]) "list of maybe neq"

    -- Eq for tuples
    assert ((1, 2) == (1, 2)) "tuple eq"
    assert ((1, 2) /= (1, 3)) "tuple neq"

    -- Eq for lists of tuples (nested container equality)
    assert ([(1, 2)] == [(1, 2)]) "list of tuple eq"
    assert ([(1, 2), (3, 4)] == [(1, 2), (3, 4)]) "list of tuple eq multi"
    assert ([(1, 2)] /= [(1, 3)]) "list of tuple neq"

    -- Show for lists
    assert (show [1, 2, 3] == "[1, 2, 3]") "show list"
    assert (show ([] :: [Integer]) == "[]") "show empty list"

    -- Show for basic types
    assert (show 42 == "42") "show int"
    assert (show True == "True") "show bool"
    assert (show False == "False") "show false"
