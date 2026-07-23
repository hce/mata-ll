-- A self-referential prime sieve over an infinite list. Exercises lazy
-- evaluation: `primes` is consumed (via takeWhile) while it is still being
-- produced. takeWhile now lives in the prelude.
primes :: [Int]
primes = 2:3:5:[x | x <- [6..], length (filter (\i -> x `mod` i == 0) $ takeWhile (\y -> y < (x `div` 2)) primes) == 0]

main :: IO ()
main = print (take 25 primes)
