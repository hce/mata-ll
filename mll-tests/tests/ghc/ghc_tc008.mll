-- GHC tc008: Typeclass instance selection
-- Tests correct dispatch on concrete vs polymorphic Show instances

class MyShow a where
    myShow :: a -> String

data Fruit = Apple | Banana | Cherry
    deriving (Eq)

instance MyShow Fruit where
    myShow Apple  = "Apple"
    myShow Banana = "Banana"
    myShow Cherry = "Cherry"

instance MyShow Integer where
    myShow n
        | n < 0     = "neg"
        | n == 0    = "zero"
        | otherwise = "pos"

instance MyShow Bool where
    myShow True  = "yes"
    myShow False = "no"

showMyList :: MyShow a => [a] -> String
showMyList xs = foldl (\acc x -> acc ++ myShow x ++ " ") "" xs

main :: IO ()
main = do
    -- Concrete instance selection
    assert (myShow Apple  == "Apple")  "fruit apple"
    assert (myShow Banana == "Banana") "fruit banana"
    assert (myShow (42 :: Integer) == "pos")  "int pos"
    assert (myShow (0  :: Integer) == "zero") "int zero"
    assert (myShow True  == "yes") "bool true"
    assert (myShow False == "no")  "bool false"

    -- Test show on list using showMyList
    assert (showMyList [Apple, Banana, Cherry] == "Apple Banana Cherry ") "show list"

    putStrLn "ok"
