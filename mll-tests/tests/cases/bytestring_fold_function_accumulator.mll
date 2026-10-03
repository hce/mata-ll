stepDigits :: (Int -> Int) -> Int -> Int -> Int
stepDigits g b x = g x * 10 + b

main :: IO ()
main = do
    let bs = bsPack [1, 2, 3]
    let k = bsFoldl (\g b x -> g x * 10 + b) (\x -> x) bs
    assert (k 0 == 123) "lambda step over a function accumulator"
    assert (k 4 == 4123) "the folded function applied again"
    assert (bsFoldl stepDigits id bs 7 == 7123) "named step, result applied in the same call"
    assert (bsFoldl (\g b x -> g x + b) (\x -> x * 2) bs 5 == 16) "accumulator applied at the call"
    let fs = bsFoldl (\acc b -> (\x -> x + b) : acc) [] bs
    assert (map (\f -> f 1) fs == [4, 3, 2]) "list of functions accumulator"
    let step = \g b -> \x -> g (x + b)
    assert (bsFoldl step id (bsPack [1, 1]) 0 == 2) "nested-lambda step"
    assert (bsFoldl (\acc b -> acc + b) 0 bs == 6) "Int accumulator"
    assert (bsUnpack (bsMap (\b -> b + 1) bs) == [2, 3, 4]) "bsMap"
    assert (bsUnpack (bsZipWith (\a b -> a * b) bs bs) == [1, 4, 9]) "bsZipWith"
