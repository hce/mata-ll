fib :: [Int]
fib = 1:1:zipWith (+) fib (tail fib)

export fibonacci :: Int -> [Int]
fibonacci = flip take fib
