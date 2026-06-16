-- Test: default methods with operator syntax

class Similar a where
    (~=) :: a -> a -> Bool
    (!~) :: a -> a -> Bool
    (!~) x y = not (x ~= y)

instance Similar Integer where
    (~=) x y = x == y

main :: IO ()
main = do
    putStrLn (show (1 ~= 1))
    putStrLn (show (1 ~= 2))
    putStrLn (show (1 !~ 1))
    putStrLn (show (1 !~ 2))
-- expect: True
-- expect: False
-- expect: False
-- expect: True
