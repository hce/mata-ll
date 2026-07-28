myLoop :: Int
    -> Int
    -> Int
    -> Int
    -> Int
myLoop initial _      _       0          = initial
myLoop initial factor modulus iterations = myLoop v factor modulus i'
  where
    v  = (initial * factor) `mod` modulus
    i' = iterations - 1

main :: IO ()
main = do
    print $ myLoop 1 2 1023 1000000
    print $ myLoop 1 2 1023 1000001
    print $ myLoop 1 2 1023 1000002
    print $ myLoop 1 2 1023 1000003
    print $ myLoop 1 2 1023 1000004
