-- GHC cgrun047: Deriving Eq on various shapes
-- Tests derived Eq for multiple ADT shapes

-- Simple enum
data Suit = Hearts | Diamonds | Clubs | Spades
    deriving (Show, Eq)

-- Record
data Point = MkPoint { px :: Integer, py :: Integer }
    deriving (Show, Eq)

-- Parameterized
data Box a = MkBox a
    deriving (Show, Eq)

-- Multiple constructors (concrete types only)
data Result = Success Integer | Failure Integer
    deriving (Show, Eq)

main :: IO ()
main = do
    -- Enum Eq
    assert (Hearts == Hearts) "suit eq"
    assert (Hearts /= Diamonds) "suit neq"
    assert (Clubs /= Spades) "suit neq2"

    -- Record Eq
    let p1 = MkPoint { px = 3, py = 4 }
    let p2 = MkPoint { px = 3, py = 4 }
    let p3 = MkPoint { px = 3, py = 5 }
    assert (p1 == p2) "point eq"
    assert (p1 /= p3) "point neq"

    -- Parameterized Eq
    assert (MkBox 42 == MkBox 42) "box eq"
    assert (MkBox 42 /= MkBox 43) "box neq"
    assert (MkBox "hello" == MkBox "hello") "box string eq"

    -- Multi-constructor Eq
    assert (Success 1 == Success 1) "result eq success"
    assert (Failure 99 == Failure 99) "result eq failure"
    assert (Success 1 /= Failure 99) "result neq"
    assert (Success 1 /= Success 2) "result neq val"

    putStrLn "ok"
