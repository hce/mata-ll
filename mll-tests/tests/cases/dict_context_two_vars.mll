-- Regression (found while goldening T13): an instance context with two
-- variables of one class, `instance (Show a, Show b) => Show (Pair a b)`,
-- had its dictionary form read `y`'s dictionary as `x`'s. The context names
-- the instance head's variables (`a`, `b`) while the method's body carries
-- the checker's own spellings (`a2114`; `_inst196` in a synthesized default
-- method), and the evidence table was keyed by the declared names — a use
-- at `b` missed it and fell to the first dictionary of the class:
-- "attempt to mod a 'string' with a 'number'" showing `Pair 1 "a"` through
-- a dictionary. The dictionary form now names each constraint after the
-- variable as the body spells it. Reached here through a polymorphic
-- recursion (the dictionary-passing route without any cap), for the
-- written method, the class-default `show`, a derived instance, and
-- `/=`'s class default over a two-variable Eq instance.
module Main where

data Pair a b = Pair a b

instance (Show a, Show b) => Show (Pair a b) where
    showsPrec d (Pair x y) = showParen (d > 10) (showString "Pair " . showsPrec 11 x . showString " " . showsPrec 11 y)

instance (Eq a, Eq b) => Eq (Pair a b) where
    (Pair x y) == (Pair u v) = x == u && y == v

data Three a b c = Three a b c deriving (Show, Eq)

data Nest a b = Leaf a b | Nest (Nest [a] [b])

deep :: (Show a, Show b) => Nest a b -> String
deep (Leaf x y) = show (Pair x y) <> " " <> show (Three y x y)
deep (Nest r) = deep r

differ :: (Eq a, Eq b) => Nest a b -> Bool
differ (Leaf x y) = Pair x y /= Pair x y || Three x y x /= Three x y x
differ (Nest r) = differ r

main :: IO ()
main = do
    putStrLn (deep (Leaf (1 :: Int) "a"))
    putStrLn (deep (Nest (Leaf [2 :: Int] ["b"])))
    putStrLn (deep (Nest (Nest (Leaf [[3 :: Int]] [["c"]]))))
    putStrLn (deep (Leaf "s" (4.5 :: Number)))
    print (differ (Leaf (1 :: Int) "a"))
    print (differ (Nest (Leaf [2 :: Int] [True])))
    print (Pair (1 :: Int) "a", Three "x" (2 :: Int) False)
