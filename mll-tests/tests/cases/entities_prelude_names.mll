import Prelude hiding (Bool(..), Maybe(..), Ordering(..), concatMap, notElem, maybe)

data Bool = False | True
    deriving (Show, Eq)

data Maybe a = Nothing | Just a
    deriving Show

data Ordering = LT | EQ | GT
    deriving (Show, Eq, Ord, Enum, Bounded)

class Weird a where
    concatMap :: a -> a
    notElem :: a -> a -> Bool

instance Weird Int where
    concatMap x = x + 1
    notElem x y = if x == y then False else True

newtype W = W { maybe :: Int }
    deriving Show

flag :: Int -> Bool
flag 0 = True
flag _ = False

pick :: Bool -> String
pick True = "yes"
pick False = "no"

safeHead :: [a] -> Maybe a
safeHead [] = Nothing
safeHead (x : _) = Just x

main :: IO ()
main = do
    print (flag 0, flag 1)
    putStrLn (pick (flag 0) <> " " <> pick (flag 2))
    print (safeHead [3 :: Int, 4], safeHead ([] :: [Int]))
    print ([minBound .. maxBound] :: [Ordering])
    print (LT < GT, compare 1 (2 :: Int))
    print (concatMap (1 :: Int), notElem (3 :: Int) 3, notElem (3 :: Int) 4)
    print [x * 10 | x <- [1, 2, 3 :: Int]]
    print (maybe (W 5), W 6)
    print (if 1 < (2 :: Int) then "builtin Bool" else "other")
