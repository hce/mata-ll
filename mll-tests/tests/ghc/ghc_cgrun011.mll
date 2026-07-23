-- GHC cgrun011: Fibonacci with accumulator
-- Tests tail recursion and Int arithmetic

fib :: Int -> Int
fib n = go n 0 1
  where
    go 0 a _ = a
    go n a b = go (n - 1) b (a + b)

main :: IO ()
main = do
    assert (fib 0 == 0) "fib 0"
    assert (fib 1 == 1) "fib 1"
    assert (fib 10 == 55) "fib 10"
    assert (fib 20 == 6765) "fib 20"
    assert (fib 30 == 832040) "fib 30"
    putStrLn (show (fib 30))
