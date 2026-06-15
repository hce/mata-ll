-- GHC cgrun030: Show instances and string conversion
-- Tests show on various types

data Direction = North | South | East | West
    deriving (Show, Eq)

data Pair a b = MkPair a b
    deriving (Show, Eq)

main :: IO ()
main = do
    assert (show 42 == "42") "show Int"
    assert (show (-7) == "-7") "show neg"
    assert (show True == "True") "show True"
    assert (show False == "False") "show False"
    assert (show [1, 2, 3] == "[1, 2, 3]") "show list"
    assert (show ([] :: [Integer]) == "[]") "show empty"
    assert (show North == "North") "show North"
    assert (show South == "South") "show South"
    assert (show (1, 2) == "(1, 2)") "show tuple"
    putStrLn "ok"
