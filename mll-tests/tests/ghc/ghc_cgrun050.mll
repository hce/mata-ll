-- GHC cgrun050: Sieve of Eratosthenes (lazy list filtering)

sieve :: [Integer] -> [Integer]
sieve [] = []
sieve (p:xs) = p : sieve (filter (\x -> x `mod` p /= 0) xs)

nats :: Integer -> [Integer]
nats n = n : nats (n + 1)

primes :: [Integer]
primes = sieve (nats 2)

main :: IO ()
main = do
    let ps = take 10 primes
    assert (ps == [2, 3, 5, 7, 11, 13, 17, 19, 23, 29]) "first 10 primes"
    assert (head primes == 2) "first prime is 2"
    assert (length (take 25 primes) == 25) "25 primes"
    -- check primality: no divisors in [2..n-1]
    let isPrime n = length (filter (\d -> n `mod` d == 0) [2..(n - 1)]) == 0
    assert (isPrime 2) "2 prime"
    assert (isPrime 29) "29 prime"
    assert (not (isPrime 4)) "4 not prime"
    putStrLn "ok"
