import Prelude hiding (concatMap, enumFrom, enumFromThen, enumFromTo, enumFromThenTo, error, (&&), (>>=), (>>), return)

concatMap :: Int -> Int
concatMap x = x + 100

enumFrom :: Int -> Int
enumFrom x = x + 200

enumFromThen :: Int -> Int -> Int
enumFromThen a b = a * b

enumFromTo :: Int -> Int -> Int
enumFromTo a b = a - b

enumFromThenTo :: Int -> Int -> Int -> Int
enumFromThenTo a b c = a + b + c

error :: Int -> Int
error x = x + 300

(&&) :: Int -> Int -> Int
(&&) a b = a * 10 + b

(>>=) :: Int -> Int -> Int
(>>=) a b = a * 100 + b

(>>) :: Int -> Int -> Int
(>>) a b = a - b

return :: Int -> Int
return x = x + 400

squares :: [Int] -> [Int]
squares xs = [x * x | x <- xs]

ordered :: [Int] -> [(Int, Int)]
ordered xs = [(x, y) | x <- xs, y <- xs, x < y]

justs :: [Maybe Int] -> [Int]
justs ms = [x + bonus | Just x <- ms, let bonus = concatMap 0]

band :: Int -> String
band n
    | n > 0, n < 10 = "digit"
    | n >= 10, even n = "even"
    | otherwise = "other"

headOr :: Int -> [Int] -> Int
headOr d xs = pick xs
  where
    pick ys
        | null ys = d
        | otherwise = (\(y : _) -> y) ys

positive :: Maybe Int -> Int
positive m
    | Just x <- m, x > 0 = x
    | otherwise = 0

addAll :: [Maybe Int] -> IO Int
addAll ms = do
    [Just a, Just b] <- pure ms
    let (c, d) = (a * 2, b * 2)
    pure (a + b + c + d)

main :: IO ()
main = do
    assert (squares [1, 2, 3] == [1, 4, 9]) "a generator"
    assert (ordered [1, 2, 3] == [(1, 2), (1, 3), (2, 3)]) "two generators and a filter"
    assert (justs [Just 1, Nothing, Just 3] == [101, 103]) "a pattern generator and a let qualifier"
    assert (take 3 [5 :: Int ..] == [5, 6, 7]) "a range from"
    assert (take 3 [1 :: Int, 3 ..] == [1, 3, 5]) "a range from-then"
    assert ([1 :: Int .. 4] == [1, 2, 3, 4]) "a range from-to"
    assert ([10 :: Int, 8 .. 1] == [10, 8, 6, 4, 2]) "a range from-then-to"
    assert (map band [5, 12, 13] == ["digit", "even", "other"]) "guard commas"
    assert (headOr 0 [4, 5] == 4) "a where guard chain and a pattern lambda"
    assert (map positive [Just 3, Just (0 - 3), Nothing] == [3, 0, 0]) "a pattern guard"
    total <- addAll [Just 1, Just 2]
    assert (total == 9) "refutable binds in a do block"
    assert (concatMap 1 == 101) "the module's concatMap"
    assert (enumFrom 1 == 201) "the module's enumFrom"
    assert (enumFromThen 3 4 == 12) "the module's enumFromThen"
    assert (enumFromTo 9 4 == 5) "the module's enumFromTo"
    assert (enumFromThenTo 1 2 3 == 6) "the module's enumFromThenTo"
    assert (error 1 == 301) "the module's error"
    assert ((2 && 3) == 23) "the module's (&&)"
    assert ((2 >>= 3) == 203) "the module's (>>=)"
    assert ((9 >> 4) == 5) "the module's (>>)"
    assert (return 1 == 401) "the module's return"
    print (squares [1, 2, 3], ordered [1, 2, 3], justs [Just 1, Nothing, Just 3])
    print (take 3 [5 :: Int ..], take 3 [1 :: Int, 3 ..], [1 :: Int .. 4], [10 :: Int, 8 .. 1])
    print (map band [5, 12, 13], headOr 0 [4, 5], map positive [Just 3, Just (0 - 3), Nothing], total)
    print (concatMap 1, enumFrom 1, enumFromThen 3 4, enumFromTo 9 4, enumFromThenTo 1 2 3)
    print (error 1, 2 && 3, 2 >>= 3, 9 >> 4, return 1)
