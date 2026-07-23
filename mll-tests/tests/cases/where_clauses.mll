tripled :: Int -> Int
tripled x = result
    where result = x + x + x

sumList :: [Int] -> Int
sumList xs = go 0 xs
    where
        go acc [] = acc
        go acc (x:rest) = go (acc + x) rest

main :: IO ()
main = do
    assert (tripled 7 == 21) "tripled 7"
    assert (tripled 0 == 0) "tripled 0"
    assert (sumList [1, 2, 3, 4, 5] == 15) "sumList [1..5]"
    assert (sumList [] == 0) "sumList []"
