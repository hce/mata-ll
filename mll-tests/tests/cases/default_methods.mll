-- Test: default method implementations in class declarations

class MyEq a where
    myEq :: a -> a -> Bool
    myNeq :: a -> a -> Bool
    myNeq x y = not (myEq x y)

-- Instance that uses the default myNeq
instance MyEq Integer where
    myEq x y = x == y

-- Instance that overrides the default myNeq
instance MyEq String where
    myEq x y = x == y
    myNeq x y = not (x == y)

main :: IO ()
main = do
    putStrLn (show (myEq 1 1))
    putStrLn (show (myEq 1 2))
    putStrLn (show (myNeq 1 1))
    putStrLn (show (myNeq 1 2))
    putStrLn (show (myEq "a" "a"))
    putStrLn (show (myNeq "a" "b"))
-- expect: True
-- expect: False
-- expect: False
-- expect: True
-- expect: True
-- expect: True
