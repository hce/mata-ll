-- Superclass contexts on class declarations: a single bare constraint
-- (`Eq a =>`) and a parenthesized list (`(Eq a, Show a) =>`). An instance
-- of the subclass requires instances of every superclass.

class Eq a => Container a where
    empty :: a
    size :: a -> Int

class (Eq a, Show a) => Pretty a where
    pretty :: a -> String
    pretty x = "<" <> show x <> ">"

data Box = Box Int
    deriving (Eq, Show)

instance Pretty Box where

data Bag = Bag Int
    deriving (Eq)

instance Container Bag where
    empty = Bag 0
    size (Bag n) = n

-- A function using a subclass constraint can reach both superclass methods.
describe :: Pretty a => a -> String
describe x = if x == x then pretty x else "impossible"

main :: IO ()
main = do
    assert (describe (Box 3) == "<Box 3>") "describe via default method"
    assert (pretty (Box 1) == "<Box 1>") "pretty default uses Show superclass"
    assert (size (empty :: Bag) == 0) "single-superclass class"
    assert (Bag 2 == Bag 2) "Eq superclass reachable"
    putStrLn "."
