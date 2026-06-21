# Bugs found by short manual programs, but not by large tests

An extensive test suite was created with claude code, including an
ImpulseTracker decoder, various cryptographic algorithms with NIST etc
tests, as well as classical CS algorithms such as a red-black tree.
While those did find many subtle bugs, others stayed undetected. This
file lists those bugs.

## non-top declaration

The following program returned (1, 1, 144, 233) instead of the
expected (144, 233, 144, 233):

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



## prime number computation

The following line found a bug in the mata-ll compiler:

    let primes = 2:3:5:[x | x <- [6..], length (filter ((== 0) . (x `mod`)) $ takeWhile (< (x `div` 2)) primes) == 0]


