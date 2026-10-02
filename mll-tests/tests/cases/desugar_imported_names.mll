import Prelude hiding (concatMap, enumFromTo, error, (&&))
import DesugarOwn

doubled :: [Int] -> [Int]
doubled xs = [x + x | x <- xs]

threeFrom :: Int -> [Int]
threeFrom n = [n .. n + 2]

headOr :: Int -> [Int] -> Int
headOr d xs = pick xs
  where
    pick ys
        | null ys = d
        | otherwise = (\(y : _) -> y) ys

size :: Int -> String
size n
    | n > 0, n < 10 = "small"
    | otherwise = "large"

main :: IO ()
main = do
    assert (doubled [1, 2, 3] == [2, 4, 6]) "a comprehension next to an imported concatMap"
    assert (concatMap (\x -> [x]) [1, 2, 3] == [3, 2, 1]) "the imported concatMap, by name"
    assert (threeFrom 5 == [5, 6, 7]) "a range next to an imported enumFromTo"
    assert (enumFromTo 5 7 == [7, 5]) "the imported enumFromTo, by name"
    assert (headOr 0 [4, 5] == 4) "guard and lambda fall-throughs next to an imported error"
    assert (error "unused" == 0 - 1) "the imported error, by name"
    assert (map size [5, 50] == ["small", "large"]) "guard commas next to an imported (&&)"
    assert ((5 && 6) == 56) "the imported (&&), by name"
    Just a <- pure (Just 1)
    [b, c, _] <- pure (threeFrom a)
    assert (b + c == 3) "refutable binds next to an imported error"
    print (doubled [1, 2, 3], concatMap (\x -> [x]) [1, 2, 3])
    print (threeFrom 5, enumFromTo 5 7)
    print (headOr 0 [4, 5], error "unused", map size [5, 50], 5 && 6)
    print (a, b, c)
