fib' :: [Integer]
fib' = [1, 1] ++ zipWith (+) fib' (drop 1 fib')

fibonacci' :: Integer -> Integer
fibonacci' = head . reverse . flip take fib'

fibonacci :: Integer -> Integer
fibonacci = head . reverse . flip take fib
  where
    fib = [1, 1] ++ zipWith (+) fib (drop 1 fib)

main :: IO ()
main = print $ ((fibonacci 12), (fibonacci 13), (fibonacci' 12), (fibonacci' 13))
