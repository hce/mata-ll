class Same a where
    same :: a -> a -> Bool

class Same a => Ranked a where
    rank :: a -> a -> Int

data Prio = Low | High

instance Same Prio where
    same Low Low = True
    same High High = True
    same _ _ = False

instance Ranked Prio where
    rank Low Low = 0
    rank Low High = -1
    rank High Low = 1
    rank High High = 0

atMost :: Ranked a => a -> a -> Bool
atMost x y = same x y || rank x y < 0

main :: IO ()
main = do
    assert (rank Low High == -1) "Low < High"
    assert (rank High High == 0) "High == High"
    assert (same Low Low) "Same Low Low"
    assert (not (same Low High)) "Same Low High"
    assert (atMost Low High) "superclass method through the subclass constraint"
    assert (not (atMost High Low)) "superclass method, other order"
