-- Number theory at Integer: factorials, powers, digit sums, gcd/lcm,
-- modular exponentiation, Miller-Rabin-free primality by trial division,
-- Catalan numbers, Collatz records at Int, and the signed division
-- families.
-- Leans on: arbitrary-precision Integer everywhere, div/mod vs quot/rem
-- on negatives, divMod/quotRem tuples, toInteger/fromInteger crossings,
-- Int loops, show of very large and negative values.
import LString

factorial :: Integer -> Integer
factorial n = product [1 .. n]

digitSum :: Integer -> Integer
digitSum 0 = 0
digitSum n = n `mod` 10 + digitSum (n `div` 10)

digitCount :: Integer -> Int
digitCount n = strLen (show (abs n))

gcd' :: Integer -> Integer -> Integer
gcd' a 0 = abs a
gcd' a b = gcd' b (a `mod` b)

lcm' :: Integer -> Integer -> Integer
lcm' _ 0 = 0
lcm' 0 _ = 0
lcm' a b = abs (a * b) `div` gcd' a b

powMod :: Integer -> Integer -> Integer -> Integer
powMod _ 0 m = 1 `mod` m
powMod b e m
    | even e = let h = powMod b (e `div` 2) m in h * h `mod` m
    | otherwise = b * powMod b (e - 1) m `mod` m

isPrime :: Integer -> Bool
isPrime n
    | n < 2 = False
    | n < 4 = True
    | even n = False
    | otherwise = go 3
  where
    go d
        | d * d > n = True
        | n `mod` d == 0 = False
        | otherwise = go (d + 2)

primeFactors :: Integer -> [Integer]
primeFactors n = go n 2
  where
    go 1 _ = []
    go k d
        | d * d > k = [k]
        | k `mod` d == 0 = d : go (k `div` d) d
        | otherwise = go k (d + 1)

catalan :: Int -> Integer
catalan n = factorial (2 * toInteger n) `div` (factorial (toInteger n + 1) * factorial (toInteger n))

collatzLength :: Int -> Int
collatzLength n = go n 1
  where
    go 1 acc = acc
    go k acc = go (if even k then k `div` 2 else 3 * k + 1) (acc + 1)

longestCollatz :: Int -> (Int, Int)
longestCollatz limit = foldl step (1, 1) [2 .. limit]
  where
    step (bestN, bestLen) n =
        let len = collatzLength n
        in if len > bestLen then (n, len) else (bestN, bestLen)

fibPair :: Int -> (Integer, Integer)
fibPair 0 = (0, 1)
fibPair n = let (a, b) = fibPair (n - 1) in (b, a + b)

isqrt :: Integer -> Integer
isqrt n = go n
  where
    go x = let y = (x + n `div` x) `div` 2 in if y >= x then x else go y

main :: IO ()
main = do
    print (factorial 30)
    print (factorial 52 `div` factorial 47)
    print (2 ^ 200, negate (3 ^ 101))
    print (digitSum (factorial 100), digitCount (factorial 100), digitCount (negate (2 ^ 300)))
    print (gcd' 1071 462, gcd' (2 ^ 64) (6 ^ 30), lcm' 21 6, lcm' 0 5, gcd' (-12) 18)
    print (powMod 2 1000 1000000007, powMod 3 (10 ^ 18) 1000000007, powMod 7 0 13)
    print (filter isPrime [1 .. 60])
    print (isPrime 1000000007, isPrime 1000000009, isPrime 1000000011)
    print (primeFactors 360, primeFactors 9999991, primeFactors (2 ^ 20 * 3 ^ 5), primeFactors 1)
    print (map catalan [0 .. 12], catalan 30)
    print (longestCollatz 1000, collatzLength 27)
    print (fst (fibPair 300))
    print (isqrt (10 ^ 40), isqrt 99, isqrt 100, isqrt 1)
    print (map (\(a, b) -> (a `div` b, a `mod` b, a `quot` b, a `rem` b)) [(7, 2), (-7, 2), (7, -2), (-7, -2)])
    print (map (\(a, b) -> (divMod a b, quotRem a b)) [(17, 5), (-17, 5), (17, -5), (-17, -5)])
    print (divMod (10 ^ 30 + 7) (10 ^ 15), quotRem (negate (10 ^ 30) - 7) (10 ^ 15))
    print (toInteger (12345678 :: Int) * 10 ^ 12, fromInteger (2 ^ 40) :: Int, negate (toInteger (2 ^ 50 :: Int)))
    print (signum (-5), abs (-5), signum (0 :: Integer), (-5) `mod` 3, (-5) `rem` 3)
    print (sum (map toInteger [1 .. 1000 :: Int]), product (map toInteger [1 .. 25 :: Int]))
    print (compare (2 ^ 100) (3 ^ 63), max (2 ^ 100) (3 ^ 63), (2 ^ 100) == (4 ^ 50))
