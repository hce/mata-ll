-- GHC tc011: Multiway if desugaring
-- Nested if-then-else chains (the manual desugaring of MultiWayIf)

classify :: Integer -> String
classify n =
    if n < 0
        then "negative"
        else if n == 0
            then "zero"
            else if n < 10
                then "small"
                else if n < 100
                    then "medium"
                    else "large"

fizzbuzz :: Integer -> String
fizzbuzz n =
    if n `mod` 15 == 0
        then "FizzBuzz"
        else if n `mod` 3 == 0
            then "Fizz"
            else if n `mod` 5 == 0
                then "Buzz"
                else "other"

-- Nested if used as an expression inside arithmetic
absoluteDiff :: Integer -> Integer -> Integer
absoluteDiff a b =
    if a > b then a - b else b - a

-- Nested if in a list comprehension result
labels :: [Integer] -> [String]
labels ns = map (\n -> if n > 0 then "pos" else if n == 0 then "zero" else "neg") ns

main :: IO ()
main = do
    assert (classify (0 - 5) == "negative") "neg"
    assert (classify 0  == "zero")   "zero"
    assert (classify 7  == "small")  "small"
    assert (classify 42 == "medium") "medium"
    assert (classify 999 == "large") "large"

    assert (fizzbuzz 15 == "FizzBuzz") "fizzbuzz 15"
    assert (fizzbuzz 9  == "Fizz")     "fizzbuzz 9"
    assert (fizzbuzz 10 == "Buzz")     "fizzbuzz 10"
    assert (fizzbuzz 7  == "other")    "fizzbuzz 7"

    assert (absoluteDiff 10 3 == 7) "absdiff 1"
    assert (absoluteDiff 3 10 == 7) "absdiff 2"
    assert (absoluteDiff 5 5  == 0) "absdiff eq"

    assert (labels [1, 0, 0 - 1] == ["pos", "zero", "neg"]) "labels"

    putStrLn "ok"
