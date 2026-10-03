module EntClassB (showB, Sized(..), Weighted(..), Crate(..)) where

class Pretty a where
    pretty :: a -> String

instance Pretty Int where
    pretty n = "B:" <> show n

instance Pretty Bool where
    pretty b = if b then "B:yes" else "B:no"

class Sized a where
    size :: a -> String

class Sized a => Weighted a where
    weight :: a -> Int

data Crate = Crate Int

instance Sized Crate where
    size (Crate n) = "crate of " <> show n

instance Weighted Crate where
    weight (Crate n) = n * 10

showB :: Int -> String
showB n = pretty n <> " " <> pretty (n > 0)
