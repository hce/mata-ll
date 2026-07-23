-- Test: default methods with operator syntax

class Similar a where
    (~=) :: a -> a -> Bool
    (!~) :: a -> a -> Bool
    (!~) x y = not (x ~= y)

instance Similar Int where
    (~=) x y = x == y

main :: IO ()
main = do
    -- `Similar` is a user (non-standard) class; an integer literal used with
    -- its operators is `(Similar a, Num a) => a`, which GHC's defaulting cannot
    -- resolve. Pin the left operand's type (the right unifies to it).
    putStrLn (show ((1 :: Int) ~= 1))
    putStrLn (show ((1 :: Int) ~= 2))
    putStrLn (show ((1 :: Int) !~ 1))
    putStrLn (show ((1 :: Int) !~ 2))
-- expect: True
-- expect: False
-- expect: False
-- expect: True
