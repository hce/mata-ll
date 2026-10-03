module EntClassA (showA, describe, Sized(..), Box(..), measureAll) where

class Pretty a where
    pretty :: a -> String

instance Pretty Int where
    pretty n = "A:" <> show n

class Shape a where
    area :: a -> Int

data Sq = Sq Int

instance Shape Sq where
    area (Sq n) = n * n

class Sized a where
    size :: a -> Int
    sizeTwice :: a -> Int
    sizeTwice x = 2 * size x

data Box = Box Int

instance Sized Box where
    size (Box n) = n

showA :: Int -> String
showA n = pretty n

describe :: Int -> Int
describe n = area (Sq n)

measureAll :: Sized a => [a] -> Int
measureAll xs = sum (map size xs)
