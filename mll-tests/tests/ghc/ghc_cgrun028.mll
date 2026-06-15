-- GHC cgrun028: Lambda expressions
-- Tests lambda syntax in various positions

add :: Integer -> Integer -> Integer
add = \x y -> x + y

mul :: Integer -> Integer -> Integer
mul = \x y -> x * y

main :: IO ()
main = do
    -- Basic lambda
    let f = \x -> x + 1
    assert (f 5 == 6) "basic lambda"

    -- Multi-arg lambda
    let g = \x y -> x * y + 1
    assert (g 3 4 == 13) "multi lambda"

    -- Lambda in map
    assert (map (\x -> x * x) [1, 2, 3, 4] == [1, 4, 9, 16]) "lambda map"

    -- Lambda in filter
    assert (filter (\x -> x > 3) [1, 2, 3, 4, 5] == [4, 5]) "lambda filter"

    -- Lambda in foldl
    let total = foldl (\a b -> a + b) 0 [1..10]
    assert (total == 55) "lambda fold"

    -- Top-level lambda bindings
    assert (add 3 4 == 7) "top lambda add"
    assert (mul 3 4 == 12) "top lambda mul"

    -- Lambda returning function (partial application style)
    let double = mul 2
    assert (map double [1, 2, 3] == [2, 4, 6]) "lambda partial"

    putStrLn "ok"
