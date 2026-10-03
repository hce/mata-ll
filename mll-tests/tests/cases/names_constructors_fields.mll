data T = A' Int | A_prime Int | A'' | A_prime_prime deriving (Show, Eq, Ord)

data R = R { x' :: Int, x_prime :: Int, end :: Int, end_ :: Int, select :: Int, local :: Int }
    deriving (Show, Eq)

newtype W' = W' Int deriving (Show, Eq)

newtype W_prime = W_prime Int deriving (Show, Eq)

data P' = P' { val' :: Int } deriving (Show, Eq, Ord)

data P_prime = P_prime { val_prime :: Int } deriving (Show, Eq, Ord)

val :: T -> Int
val (A' n) = n
val (A_prime n) = n * 100
val A'' = 7
val A_prime_prime = 8

total :: R -> Int
total r = x' r + x_prime r * 10 + end r * 100 + end_ r * 1000 + select r * 10000 + local r * 100000

unwrap :: W' -> W_prime -> Int
unwrap (W' a) (W_prime b) = a * 10 + b

main :: IO ()
main = do
    k <- pure (1 :: Int)
    print (A' k, A_prime k, A'', A_prime_prime)
    print (val (A' k) + val (A_prime 2), val A'', val A_prime_prime)
    print (A' k == A_prime k, compare (A' k) (A_prime k), A' k < A'')
    print (map val [A' 3, A_prime 3])
    let r = R { x' = k, x_prime = 2, end = 3, end_ = 4, select = 5, local = 6 }
    print r
    print (total r, total (r { x' = 9 }), total (r { x_prime = 9, end_ = 0 }))
    print (map (\f -> f r) [x', x_prime, end, end_, select, local])
    print (W' k, W_prime k, unwrap (W' k) (W_prime 2))
    print (P' k, P_prime k, val' (P' 5) + val_prime (P_prime 6))
    print (P' k == P' 1, compare (P_prime 1) (P_prime 2), show (P' 2) == show (P_prime 2))
