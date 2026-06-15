-- GHC cgrun039: QuickSort
-- Tests list manipulation, guards, list comprehensions, recursion

appendList :: [a] -> [a] -> [a]
appendList [] ys = ys
appendList (x:xs) ys = x : appendList xs ys

qsort :: [Integer] -> [Integer]
qsort [] = []
qsort (p:xs) = appendList (qsort smaller) (p : qsort bigger)
  where
    smaller = [x | x <- xs, x < p]
    bigger  = [x | x <- xs, x >= p]

main :: IO ()
main = do
    assert (qsort ([] :: [Integer]) == []) "sort empty"
    assert (qsort [1] == [1]) "sort single"
    assert (qsort [3, 1, 2] == [1, 2, 3]) "sort 3"
    assert (qsort [5, 3, 8, 1, 9, 2, 7, 4, 6] == [1, 2, 3, 4, 5, 6, 7, 8, 9]) "sort 9"
    assert (qsort [1, 1, 1] == [1, 1, 1]) "sort dups"
    assert (qsort [5, 4, 3, 2, 1] == [1, 2, 3, 4, 5]) "sort reverse"
    assert (qsort [1, 2, 3, 4, 5] == [1, 2, 3, 4, 5]) "sort sorted"

    -- Sort preserves length
    assert (length (qsort [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5]) == 11) "sort preserves length"

    putStrLn "ok"
