-- Stress test: deep recursion to test stack handling

-- Tail-recursive countdown from N to 0
countdown :: Integer -> Integer
countdown 0 = 0
countdown n = countdown (n - 1)

-- Accumulator-style sum: 1+2+...+n
sumAcc :: Integer -> Integer -> Integer
sumAcc 0 acc = acc
sumAcc n acc = sumAcc (n - 1) (acc + n)

-- Build a list of length n
buildList :: Integer -> [Integer]
buildList 0 = []
buildList n = n : buildList (n - 1)

-- Length of a list (recursive)
myLength :: [Integer] -> Integer
myLength [] = 0
myLength (_:xs) = 1 + myLength xs

-- Mutual recursion: even/odd
myEven :: Integer -> Bool
myEven 0 = True
myEven n = myOdd (n - 1)

myOdd :: Integer -> Bool
myOdd 0 = False
myOdd n = myEven (n - 1)

-- Fibonacci with memoization via list building
fibList :: Integer -> [Integer]
fibList n = fibHelper 0 1 n

fibHelper :: Integer -> Integer -> Integer -> [Integer]
fibHelper _ _ 0 = []
fibHelper a b n = a : fibHelper b (a + b) (n - 1)

main :: IO ()
main = do
    assert (countdown 10000 == 0) "countdown 10000"
    assert (sumAcc 5000 0 == 12502500) "sumAcc 5000"
    let big = buildList 3000
    assert (myLength big == 3000) "buildList 3000"
    assert (myEven 200 == True) "even 200"
    assert (myOdd 201 == True) "odd 201"
    let fibs = fibList 30
    assert (myLength fibs == 30) "fib list length"
    putStrLn "ok"
