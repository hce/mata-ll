module Main where

-- T4 (2026-10 fresh-eyes review): a dictionary-passing function with two
-- constraints of ONE class took two dictionary parameters of one name
-- (`__dict_Show`), so the second shadowed the first and both `show`s
-- read the second dictionary (`B/B` for `A/B`, a crash at `Int`/String).
-- Each constraint now has its own positional parameter. Shapes: two and
-- three constraints of one class, two classes on one variable, a
-- constraint on a variable that only occurs nested, and the constraints
-- declared in the opposite order to the parameters. Every function is
-- polymorphically recursive over the dead `d`, which forces the fallback.

data A = A deriving (Show, Eq)
data B = B deriving (Show)
data C = C deriving (Show)

both :: (Show a, Show b) => Int -> d -> a -> b -> String
both 0 _ x y = show x <> "/" <> show y
both n z x y = both (n - 1) [z] x y

three :: (Show a, Show b, Show c) => Int -> d -> a -> b -> c -> String
three 0 _ x y z = show x <> "/" <> show y <> "/" <> show z
three n w x y z = three (n - 1) [w] x y z

sameVar :: (Show a, Eq a) => Int -> d -> a -> a -> String
sameVar 0 _ x y = show x <> (if x == y then "=" else "/=") <> show y
sameVar n w x y = sameVar (n - 1) [w] x y

nested :: Show a => Int -> d -> [a] -> String
nested 0 _ xs = show xs
nested n w xs = nested (n - 1) [w] xs

swapped :: (Show b, Show a) => Int -> d -> a -> b -> String
swapped 0 _ x y = show x <> "<" <> show y
swapped n w x y = swapped (n - 1) [w] x y

main :: IO ()
main = do
    putStrLn (both 20 () A B)
    putStrLn (both 20 () (1 :: Int) "s")
    putStrLn (three 20 () A B C)
    putStrLn (three 20 () (1 :: Int) "s" True)
    putStrLn (sameVar 20 () A A)
    putStrLn (sameVar 20 () (1 :: Int) 2)
    putStrLn (nested 20 () [A, A])
    putStrLn (nested 20 () [1 :: Int, 2])
    putStrLn (swapped 20 () A "b")
    putStrLn (swapped 20 () (3 :: Int) B)

-- expect: A/B
-- expect: 1/"s"
-- expect: A/B/C
-- expect: 1/"s"/True
-- expect: A=A
-- expect: 1/=2
-- expect: [A,A]
-- expect: [1,2]
-- expect: A<"b"
-- expect: 3<B
