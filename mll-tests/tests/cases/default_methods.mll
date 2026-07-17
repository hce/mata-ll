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
    -- `MyEq` is a user (non-standard) class, so an integer literal used as its
    -- argument is `(MyEq a, Num a) => a` — ambiguous under GHC's defaulting
    -- rule (which only defaults standard classes). Annotate to pin `a`.
    putStrLn (show (myEq (1 :: Integer) 1))
    putStrLn (show (myEq (1 :: Integer) 2))
    putStrLn (show (myNeq (1 :: Integer) 1))
    putStrLn (show (myNeq (1 :: Integer) 2))
    putStrLn (show (myEq "a" "a"))
    putStrLn (show (myNeq "a" "b"))
-- expect: True
-- expect: False
-- expect: False
-- expect: True
-- expect: True
-- expect: True
