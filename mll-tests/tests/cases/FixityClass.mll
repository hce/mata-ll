module FixityClass (Mix(..), mixed, blended) where

infixr 5 <+>
infixr 5 `blend`

class Mix a where
    (<+>) :: a -> a -> a
    blend :: a -> a -> a

instance Mix Int where
    a <+> b = a - b
    blend a b = a - b

mixed :: Int
mixed = 10 <+> 3 <+> 2

blended :: Int
blended = 10 `blend` 3 `blend` 2
