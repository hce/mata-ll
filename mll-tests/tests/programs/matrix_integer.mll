-- Integer matrices: multiplication, transpose, powers by repeated squaring
-- (Fibonacci numbers through the Q-matrix), determinants by Laplace
-- expansion, and Gauss-style row reduction over Integer fractions kept as
-- numerator/denominator pairs.
-- Leans on: arbitrary-precision Integer arithmetic on nested lists,
-- zipWith/transpose patterns, recursion over Int exponents, show of large
-- and negative Integers, a user Num instance for a rational type.
type Matrix = [[Integer]]

transpose' :: [[a]] -> [[a]]
transpose' [] = []
transpose' ([] : _) = []
transpose' xss = map head xss : transpose' (map tail xss)

mmul :: Matrix -> Matrix -> Matrix
mmul a b = [[sum (zipWith (*) row col) | col <- transpose' b] | row <- a]

identity :: Int -> Matrix
identity n = [[if i == j then 1 else 0 | j <- [1 .. n]] | i <- [1 .. n]]

mpow :: Matrix -> Int -> Matrix
mpow m 0 = identity (length m)
mpow m k
    | even k = let h = mpow m (k `div` 2) in mmul h h
    | otherwise = mmul m (mpow m (k - 1))

fib :: Int -> Integer
fib n = head (mpow [[1, 1], [1, 0]] n) !! 1

minor :: Matrix -> Int -> Matrix
minor m j = [dropAt j row | row <- tail m]
  where dropAt k row = take k row ++ drop (k + 1) row

det :: Matrix -> Integer
det [] = 1
det [[x]] = x
det m = sum [sign j * (head m !! j) * det (minor m j) | j <- [0 .. length m - 1]]
  where sign j = if even j then 1 else -1

-- Rationals as reduced numerator/denominator pairs with a Num instance.
data Ratio = Ratio Integer Integer

gcd' :: Integer -> Integer -> Integer
gcd' a 0 = abs a
gcd' a b = gcd' b (a `mod` b)

mkRatio :: Integer -> Integer -> Ratio
mkRatio n d
    | d == 0 = error "zero denominator"
    | d < 0 = mkRatio (negate n) (negate d)
    | otherwise = let g = gcd' n d in Ratio (n `div` g) (d `div` g)

instance Show Ratio where
    show (Ratio n 1) = show n
    show (Ratio n d) = show n <> "/" <> show d

instance Eq Ratio where
    (==) (Ratio a b) (Ratio c d) = a == c && b == d

instance Num Ratio where
    (+) (Ratio a b) (Ratio c d) = mkRatio (a * d + c * b) (b * d)
    (-) (Ratio a b) (Ratio c d) = mkRatio (a * d - c * b) (b * d)
    (*) (Ratio a b) (Ratio c d) = mkRatio (a * c) (b * d)
    negate (Ratio a b) = Ratio (negate a) b
    abs (Ratio a b) = Ratio (abs a) b
    signum (Ratio a _) = Ratio (signum a) 1
    fromInteger n = Ratio n 1

recipR :: Ratio -> Ratio
recipR (Ratio a b) = mkRatio b a

isZero :: Ratio -> Bool
isZero (Ratio a _) = a == 0

-- Determinant by row reduction over Ratio, to cross-check Laplace.
detGauss :: [[Ratio]] -> Ratio
detGauss [] = 1
detGauss rows = case break' (not . isZero . head) rows of
    (_, []) -> 0
    (before, pivotRow : after) ->
        let pivot = head pivotRow
            others = before ++ after
            eliminate row = zipWith (\x p -> x - p * (head row * recipR pivot)) (tail row) (tail pivotRow)
            sign = if even (length before) then 1 else negate 1
        in sign * pivot * detGauss (map eliminate others)
  where
    break' p xs = span (not . p) xs

toRatios :: Matrix -> [[Ratio]]
toRatios = map (map fromInteger)

showMatrix :: Matrix -> String
showMatrix m = mconcat [show row <> "\n" | row <- m]

main :: IO ()
main = do
    let a = [[1, 2, 3], [4, 5, 6], [7, 8, 10]]
        b = [[2, 0, 1], [1, 3, 0], [0, 1, 4]]
    putStr (showMatrix (mmul a b))
    putStr (showMatrix (transpose' a))
    putStr (showMatrix (mpow b 5))
    print (map fib [0 .. 15])
    print (fib 200)
    print (det a, det b, det (mmul a b), det a * det b)
    print (det (identity 6), det [[0, 1], [1, 0]], det [[2, -3], [-4, 5]])
    print (detGauss (toRatios a), detGauss (toRatios b), detGauss (toRatios [[1, 2], [2, 4]]))
    print (mkRatio 6 8 + mkRatio 1 6, mkRatio 3 4 * mkRatio (-2) 9, mkRatio 1 3 - 1)
    print (sum (map fromInteger [1 .. 10]) == (55 :: Ratio), negate (mkRatio 5 (-10)))
    print (det (mpow [[2, 1], [1, 1]] 40))
    print (product [1 .. 30], 2 ^ 130 - 1, negate (3 ^ 80))
