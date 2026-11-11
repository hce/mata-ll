-- Helper module for qualified_lexemes.mll: operators with their own
-- fixities, functions used backtick-infix, a plain constructor and a record
-- constructor, all reached through an import alias.
module QualOps (plus, minus, (<+>), (<&&>), Shape(..), Pair(..), area) where

plus :: Int -> Int -> Int
plus a b = a + b

minus :: Int -> Int -> Int
minus a b = a - b

infixl 6 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a * 10 + b

infixr 3 <&&>
(<&&>) :: Bool -> Bool -> Bool
a <&&> b = a && b

data Shape = Circle Int | Rect Int Int deriving (Show, Eq)

data Pair = Pair { first :: Int, second :: Int } deriving Show

area :: Shape -> Int
area (Circle r) = 3 * r * r
area (Rect w h) = w * h
