-- Infinite streams: primes, Fibonacci, Hamming numbers, Collatz orbits,
-- Pascal's triangle, and a few knots tied through laziness.
-- Leans on: top-level lazy CAF lists, zipWith over a self-reference,
-- iterate/takeWhile/cycle/repeat, short-circuiting folds over infinite
-- input, unfoldr and scanl from Data.List, unevaluated bottoms.
import Data.List (unfoldr, scanl)

primes :: [Integer]
primes = sieve [2 ..]
  where
    sieve [] = []
    sieve (p : xs) = p : sieve [x | x <- xs, x `mod` p /= 0]

fibs :: [Integer]
fibs = 0 : 1 : zipWith (+) fibs (tail fibs)

-- Every 2^a 3^b 5^c in order: merge three scaled copies of the stream.
hamming :: [Integer]
hamming = 1 : merge3 (map (* 2) hamming) (map (* 3) hamming) (map (* 5) hamming)

merge3 :: [Integer] -> [Integer] -> [Integer] -> [Integer]
merge3 xs ys zs = merge xs (merge ys zs)

merge :: [Integer] -> [Integer] -> [Integer]
merge [] ys = ys
merge xs [] = xs
merge (x : xs) (y : ys)
    | x < y = x : merge xs (y : ys)
    | x > y = y : merge (x : xs) ys
    | otherwise = x : merge xs ys

collatz :: Integer -> [Integer]
collatz n = takeWhile (/= 1) (iterate step n) ++ [1]
  where step k = if even k then k `div` 2 else 3 * k + 1

pascal :: [[Integer]]
pascal = iterate (\row -> zipWith (+) (0 : row) (row ++ [0])) [1]

digits :: Integer -> [Integer]
digits 0 = [0]
digits n = reverse (unfoldr (\k -> if k == 0 then Nothing else Just (k `mod` 10, k `div` 10)) n)

triangulars :: [Integer]
triangulars = scanl (+) 1 [2 ..]

pythagorean :: Int -> [(Int, Int, Int)]
pythagorean n = [(a, b, c) | c <- [1 .. n], b <- [1 .. c], a <- [1 .. b], a * a + b * b == c * c]

main :: IO ()
main = do
    print (take 15 primes)
    print (primes !! 100)
    print (take 15 fibs)
    print (fibs !! 90)
    print (take 20 hamming)
    print (hamming !! 200)
    print (collatz 27 == collatz 27, length (collatz 27), maximum (collatz 27))
    print (take 6 (map length (map collatz [1 .. 6])))
    mapM_ print (take 6 pascal)
    print (digits 9081726354)
    print (sum (digits (2 ^ 100)))
    print (take 10 triangulars)
    print (takeWhile (< 60) (map (^ 2) [1 ..]))
    print (take 7 (cycle [1, 2, 3]))
    print (iterate (* 2) 1 !! 20)
    print (head (filter (\n -> n * n > 1000) [1 ..]))
    print (any (> 100) [1 ..])
    print (or (repeat True))
    print (elem 97 primes)
    print (takeWhile (< 40) (dropWhile (< 20) primes))
    print (fst (span (< 5) [1 ..]))
    print (pythagorean 20)
    print (zip [1 ..] (take 4 (drop 10 primes)))
    -- Laziness proper: bottoms that are never demanded stay quiet.
    let boom = error "never forced"
    print (fst (1, boom))
    print (length (take 3 (repeat undefined)))
    print (length [undefined, undefined])
    let knot = 1 : map (* 2) knot
    print (take 8 knot)
    print (take 5 (filter even (map (\n -> n * n + 1) [1 ..])))
    print (foldr (\x acc -> x : take 2 acc) [] [1 ..])
