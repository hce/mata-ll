-- GHC cgrun027: GCD and integer arithmetic
-- Tests recursive arithmetic, mod, div

gcd_ :: Int -> Int -> Int
gcd_ a 0 = a
gcd_ a b = gcd_ b (a `mod` b)

lcm_ :: Int -> Int -> Int
lcm_ a b = (a * b) `div` gcd_ a b

isPrime :: Int -> Bool
isPrime n
    | n < 2     = False
    | n == 2    = True
    | otherwise = go 2
  where
    go d
        | d * d > n      = True
        | n `mod` d == 0 = False
        | otherwise      = go (d + 1)

pow_ :: Int -> Int -> Int
pow_ _ 0 = 1
pow_ b 1 = b
pow_ b n = if n `mod` 2 == 0
    then let half = pow_ b (n `div` 2) in half * half
    else b * pow_ b (n - 1)

main :: IO ()
main = do
    assert (gcd_ 12 8 == 4) "gcd 12 8"
    assert (gcd_ 17 13 == 1) "gcd primes"
    assert (gcd_ 100 0 == 100) "gcd x 0"
    assert (lcm_ 4 6 == 12) "lcm 4 6"
    assert (lcm_ 3 5 == 15) "lcm 3 5"

    let primes = filter isPrime [2..30]
    assert (primes == [2, 3, 5, 7, 11, 13, 17, 19, 23, 29]) "primes to 30"

    assert (pow_ 2 10 == 1024) "2^10"
    assert (pow_ 3 5 == 243) "3^5"

    putStrLn "ok"
