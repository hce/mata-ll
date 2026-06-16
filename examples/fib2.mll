fib :: [Integer]
fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)

fibonacci :: Integer -> [Integer]
fibonacci = flip take fib

main :: IO ()
main = print $ fibonacci 12
