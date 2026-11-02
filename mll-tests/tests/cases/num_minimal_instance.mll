-- GHC's Num has the mutual defaults `negate x = 0 - x` and
-- `x - y = x + negate y`: an instance gives either one and the other
-- follows, through every spelling — the method, the operator, prefix
-- minus, a section, a where-bound use, a polymorphic function.

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

mirror :: Num a => a -> a
mirror x = x - negate x

offsets :: Num a => a -> [a]
offsets x = map (subtract x) [x, negate x, 0]

diff :: Num a => a -> a
diff v = half - negate half
  where half = v - v - v

main :: IO ()
main = do
    print (negate (A 3), A 1 - A 4, - A 2, (- 5) :: A)
    print (negate (B 3), B 1 - B 4, - B 2, (- 5) :: B)
    print (mirror (A 7), mirror (B 7), mirror (7 :: Int))
    print (offsets (A 2), offsets (B 2))
    print (map (subtract 1) [A 1, A 2], map (\x -> 10 - x) [B 1, B 2])
    print (diff (A 9), diff (B 9), abs (negate (A 6)), signum (B 0 - B 4))
    print (sum [A 1, negate (A 2), A 3 - A 4], sum [B 1, negate (B 2), B 3 - B 4])
