-- Tests for record update syntax: expr { field = newVal }

data Person = Person { personName :: String, personAge :: Int }
    deriving (Show, Eq)

data Point = Point { pointX :: Int, pointY :: Int }
    deriving (Show, Eq)

main :: IO ()
main = do
    -- Basic record update
    let alice = Person { personName = "Alice", personAge = 30 }
    let older = alice { personAge = 31 }
    assert (personAge older == 31) "record update: age"
    assert (personName older == "Alice") "record update: name preserved"

    -- Update multiple fields
    let bob = alice { personName = "Bob", personAge = 25 }
    assert (personName bob == "Bob") "record update: multiple name"
    assert (personAge bob == 25) "record update: multiple age"

    -- Update on point
    let p = Point { pointX = 10, pointY = 20 }
    let p2 = p { pointX = 99 }
    assert (pointX p2 == 99) "record update: point x"
    assert (pointY p2 == 20) "record update: point y preserved"

    -- Original is unchanged
    assert (personAge alice == 30) "record update: original unchanged"
    assert (pointX p == 10) "record update: original point unchanged"

    -- Chained updates
    let p3 = p { pointX = 1 } { pointY = 2 }
    assert (pointX p3 == 1) "record update: chained x"
    assert (pointY p3 == 2) "record update: chained y"
