-- Minimal ST monad example
-- Demonstrates mutable arrays inside pure computations via runST

import Data.List (drop, replicate)

-- Sum a list using a mutable accumulator
sumST :: [Int] -> Int
sumST xs = runST (do
    acc <- newSTArrayFromList [0]
    sumGo acc xs
    readSTArray acc 0)

sumGo :: STArray s -> [Int] -> ST s ()
sumGo acc [] = return ()
sumGo acc (x:rest) = do
    cur <- readSTArray acc 0
    writeSTArray acc 0 (cur + x)
    sumGo acc rest

-- Reverse an array in place
reverseArray :: [Int] -> [Int]
reverseArray xs = runST (do
    arr <- newSTArrayFromList xs
    let n = length xs
    revLoop arr 0 (n - 1)
    stArrayToList arr)

revLoop :: STArray s -> Int -> Int -> ST s ()
revLoop arr lo hi
  | lo >= hi  = return ()
  | otherwise = do
        a <- readSTArray arr lo
        b <- readSTArray arr hi
        writeSTArray arr lo b
        writeSTArray arr hi a
        revLoop arr (lo + 1) (hi - 1)

-- Build a histogram of values 0..9
histogram :: [Int] -> [Int]
histogram xs = runST (do
    bins <- newSTArrayFromList (replicate 10 0)
    countAll bins xs
    stArrayToList bins)

countAll :: STArray s -> [Int] -> ST s ()
countAll bins [] = return ()
countAll bins (x:rest) = do
    cur <- readSTArray bins x
    writeSTArray bins x (cur + 1)
    countAll bins rest

-- Prefix sums (scan) using mutable state
prefixSums :: [Int] -> [Int]
prefixSums xs = runST (do
    let n = length xs
    src <- newSTArrayFromList xs
    dst <- newSTArrayFromList (replicate n 0)
    acc <- newSTArrayFromList [0]
    scanGo src dst acc n 0
    stArrayToList dst)

scanGo :: STArray s -> STArray s -> STArray s -> Int -> Int -> ST s ()
scanGo src dst acc n i
  | i >= n    = return ()
  | otherwise = do
        cur <- readSTArray acc 0
        val <- readSTArray src i
        let total = cur + val
        writeSTArray acc 0 total
        writeSTArray dst i total
        scanGo src dst acc n (i + 1)

main :: IO ()
main = do
    -- sumST
    assert (sumST [] == 0) "sum empty"
    assert (sumST [1, 2, 3, 4, 5] == 15) "sum 1..5"
    assert (sumST [10, -3, 7] == 14) "sum mixed"
    putStrLn "sumST: OK"

    -- reverseArray
    assert (reverseArray [] == []) "rev empty"
    assert (reverseArray [1] == [1]) "rev single"
    assert (reverseArray [1, 2, 3, 4, 5] == [5, 4, 3, 2, 1]) "rev 1..5"
    putStrLn "reverseArray: OK"

    -- histogram
    assert (histogram [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] == [1, 1, 1, 1, 1, 1, 1, 1, 1, 1]) "hist uniform"
    assert (histogram [0, 0, 0, 1, 1, 2] == [3, 2, 1, 0, 0, 0, 0, 0, 0, 0]) "hist skewed"
    putStrLn "histogram: OK"

    -- prefixSums
    assert (prefixSums [1, 2, 3, 4] == [1, 3, 6, 10]) "prefix sums"
    assert (prefixSums [5] == [5]) "prefix single"
    putStrLn "prefixSums: OK"

    putStrLn "All ST examples passed!"
