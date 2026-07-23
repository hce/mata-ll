-- GHC cgrun023: Newtype and record accessors
-- Tests newtypes and record field access

data Person = Person
    { personName :: String
    , personAge  :: Int
    }
    deriving (Show)

greet :: Person -> String
greet p = "Hello, " <> personName p <> "! You are " <> show (personAge p)

data Wrapper = Wrapper Int
    deriving (Show, Eq)

getVal :: Wrapper -> Int
getVal (Wrapper x) = x

main :: IO ()
main = do
    let alice = Person { personName = "Alice", personAge = 30 }
    assert (personName alice == "Alice") "name"
    assert (personAge alice == 30) "age"
    assert (greet alice == "Hello, Alice! You are 30") "greet"

    let w = Wrapper 25
    assert (getVal w == 25) "getVal"
    assert (w == Wrapper 25) "Wrapper eq"
    assert (w /= Wrapper 26) "Wrapper neq"
    assert (show w == "Wrapper 25") "show Wrapper"

    putStrLn "ok"
