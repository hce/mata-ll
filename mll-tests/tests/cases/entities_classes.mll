import EntClassA
import EntClassB (Weighted(..), Crate(..), showB)
import qualified EntClassB as B

class Shape a where
    area :: a -> String

data Circle = Circle

instance Shape Circle where
    area Circle = "round"

class Pretty a where
    pretty :: a -> String

instance Pretty Int where
    pretty n = "Main:" <> show n

data Bag = Bag [Int]

instance Sized Bag where
    size (Bag xs) = length xs

heavy :: Weighted a => a -> String
heavy x = B.size x <> " weighing " <> show (weight x)

main :: IO ()
main = do
    print (describe 4)
    putStrLn (area Circle)
    putStrLn (showA 1)
    putStrLn (showB 1)
    putStrLn (pretty (2 :: Int))
    print (size (Box 3), sizeTwice (Box 3), measureAll [Box 1, Box 2])
    print (size (Bag [1, 2, 3]), sizeTwice (Bag [1]), measureAll [Bag [1], Bag [2, 3]])
    putStrLn (B.size (Crate 2))
    putStrLn (heavy (Crate 4))
