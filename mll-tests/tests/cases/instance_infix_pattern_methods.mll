-- Infix method definitions whose left operand is a constructor pattern —
-- `(K a) + (K b) = …`, the common spelling of a wrapper's Num/Semigroup
-- instance — in instance bodies, class default bodies, and with backticks.
-- The member parser accepted only `x + y = …` (a plain identifier on the
-- left) and reported "Expected operator in instance method" for these.

newtype K = K Int deriving (Show, Eq)
instance Num K where
    (K a) + (K b) = K (a + b)
    (K a) - (K b) = K (a - b)
    (K a) * (K b) = K (a * b)
    negate (K a) = K (negate a)
    abs (K a) = K (abs a)
    signum (K a) = K (signum a)
    fromInteger i = K (fromInteger i)

data Two = Two Int Int deriving Show
instance Semigroup Two where
    (Two a b) <> (Two c d) = Two (a + c) (b + d)

class Combine a where
    (<+>) :: a -> a -> a
    orElse :: a -> a -> a
    orElse x _ = x

instance Combine K where
    (K a) <+> (K b) = K (a * 10 + b)
    (K x) `orElse` (K y) = if x == 0 then K y else K x

instance Combine Two where
    (<+>) (Two a b) (Two c d) = Two (a * c) (b * d)

main :: IO ()
main = do
    print (K 1 + K 2 * K 3 - K 4)
    print (Two 1 2 <> Two 3 4)
    print (K 4 <+> K 5, Two 2 3 <+> Two 4 5)
    print (K 0 `orElse` K 9, K 1 `orElse` K 9, Two 1 1 `orElse` Two 2 2)
