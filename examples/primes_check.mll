takeWhile :: (a -> Bool) -> [a] -> [a]
takeWhile _ [] = []
takeWhile p (x:xs) = if p x then x : takeWhile p xs else []

primes :: [Integer]
primes = 2:3:5:[x | x <- [6..], length (filter (\i -> x `mod` i == 0) $ takeWhile (\y -> y < (x `div` 2)) primes) == 0]

main :: IO ()
main = print (take 25 primes)
