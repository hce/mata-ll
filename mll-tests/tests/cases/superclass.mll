class Eq a where
    (==) :: a -> a -> Bool

class Eq a => Ord a where
    compare :: a -> a -> Int

data Prio = Low | High
    deriving Eq

instance Ord Prio where
    compare Low Low = 0
    compare Low High = -1
    compare High Low = 1
    compare High High = 0

main :: IO ()
main = do
    assert (compare Low High == -1) "Low < High"
    assert (compare High High == 0) "High == High"
    assert (Low == Low) "Eq Low Low"
    assert (not (Low == High)) "Eq Low High"
