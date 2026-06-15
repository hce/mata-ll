-- GHC ds009: Where clause with multiple helpers calling each other

-- Collatz sequence length, using where helpers
collatzLen :: Integer -> Integer
collatzLen n = go n 0
  where
    go 1 acc = acc + 1
    go x acc
        | x `mod` 2 == 0 = go (x `div` 2) (acc + 1)
        | otherwise       = go (3 * x + 1) (acc + 1)

-- Run-length encoding using where helpers
rleEncode :: [Integer] -> [(Integer, Integer)]
rleEncode []     = []
rleEncode (x:xs) = encode x 1 xs
  where
    encode cur cnt []     = [(cur, cnt)]
    encode cur cnt (y:ys)
        | y == cur  = encode cur (cnt + 1) ys
        | otherwise = (cur, cnt) : encode y 1 ys

dropN :: Integer -> [a] -> [a]
dropN n ys = if n == 0 then ys else dropNHelper ys (n - 1)

dropNHelper :: [a] -> Integer -> [a]
dropNHelper [] k     = []
dropNHelper (_:ys) 0 = ys
dropNHelper (_:ys) k = dropNHelper ys (k - 1)

mergeSorted :: [Integer] -> [Integer] -> [Integer]
mergeSorted [] ys         = ys
mergeSorted xs []         = xs
mergeSorted (a:as) (b:bs)
    | a <= b    = a : mergeSorted as (b:bs)
    | otherwise = b : mergeSorted (a:as) bs

-- Merge sort
mergeSort :: [Integer] -> [Integer]
mergeSort xs = mergeSortGo xs

mergeSortGo :: [Integer] -> [Integer]
mergeSortGo [] = []
mergeSortGo xs =
    let n = length xs
    in if n == 1
       then xs
       else let half = n `div` 2
            in mergeSorted (mergeSortGo (take half xs)) (mergeSortGo (dropN half xs))

main :: IO ()
main = do
    assert (collatzLen 1  == 1)  "collatz 1"
    assert (collatzLen 2  == 2)  "collatz 2"
    assert (collatzLen 6  == 9)  "collatz 6"
    assert (collatzLen 27 == 112) "collatz 27"

    assert (rleEncode [1,1,2,3,3,3,2] == [(1,2),(2,1),(3,3),(2,1)]) "rle 1"
    assert (rleEncode [5,5,5] == [(5,3)]) "rle all same"
    assert (rleEncode ([] :: [Integer]) == []) "rle empty"

    assert (mergeSort [3,1,4,1,5,9,2,6] == [1,1,2,3,4,5,6,9]) "mergesort"
    assert (mergeSort [] == ([] :: [Integer])) "mergesort empty"
    assert (mergeSort [1] == [1]) "mergesort single"

    putStrLn "ok"
