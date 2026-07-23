-- Guards with backtick infix operators

fizzbuzz :: Int -> String
fizzbuzz n
    | n `mod` 15 == 0 = "FizzBuzz"
    | n `mod` 3 == 0  = "Fizz"
    | n `mod` 5 == 0  = "Buzz"
    | otherwise       = show n

main :: IO ()
main = do
    assert (fizzbuzz 15 == "FizzBuzz") "fizzbuzz 15"
    assert (fizzbuzz 3 == "Fizz") "fizzbuzz 3"
    assert (fizzbuzz 5 == "Buzz") "fizzbuzz 5"
    assert (fizzbuzz 7 == "7") "fizzbuzz 7"
    assert (fizzbuzz 30 == "FizzBuzz") "fizzbuzz 30"
    assert (fizzbuzz 9 == "Fizz") "fizzbuzz 9"
    assert (fizzbuzz 1 == "1") "fizzbuzz 1"
