-- A builtin class default reached through evidence rather than a
-- specialization: an existential's captured dictionary and a
-- dictionary-passing body both read the default (`negate` from `-`,
-- `-` from `negate`, `foldl` from `foldr`, `/=` from `==`) as a method
-- entry of the instance's dictionary.

data A = A Int deriving (Show, Eq)

instance Num A where
    (+) (A x) (A y) = A (x + y)
    (*) (A x) (A y) = A (x * y)
    (-) (A x) (A y) = A (x - y)
    abs (A x) = A (abs x)
    signum (A x) = A (signum x)
    fromInteger n = A (fromInteger n)

data B = B Int deriving (Show, Eq)

instance Num B where
    (+) (B x) (B y) = B (x + y)
    (*) (B x) (B y) = B (x * y)
    negate (B x) = B (negate x)
    abs (B x) = B (abs x)
    signum (B x) = B (signum x)
    fromInteger n = B (fromInteger n)

data Box = forall a. (Num a, Show a, Eq a) => Box a

describe :: Box -> String
describe (Box x) = show (negate x, x - 1, x /= x + 1, x == 0 - negate x)

data Pair a = Pair a a deriving Show

instance Foldable Pair where
    foldr f z (Pair x y) = f x (f y z)

data Folded = forall t. Foldable t => Folded (t Int)

collect :: Folded -> ([Int], Int)
collect (Folded c) = (foldl (flip (:)) [] c, foldl' (+) 0 c)

sumBoxes :: [Box] -> [String]
sumBoxes bs = map describe bs

main :: IO ()
main = do
    mapM_ (putStrLn . describe) [Box (A 3), Box (B 3), Box (4 :: Int), Box (5 :: Integer)]
    print (collect (Folded (Pair 1 2)), collect (Folded [3, 4, 5]), collect (Folded (Just 6)))
    print (sumBoxes [Box (A 1), Box (B 2)])
