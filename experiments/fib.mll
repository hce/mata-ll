fib :: [Integer]
fib = 1:1:zipWith (+) fib (tail fib)

export fibonacci :: Integer -> [Integer]
fibonacci = flip take fib
