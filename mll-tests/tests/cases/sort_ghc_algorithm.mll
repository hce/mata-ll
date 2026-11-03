-- `sort` and `sortBy` are GHC 9.14.1's Data.OldList algorithm (base 4.22:
-- monotone-run detection, the fused three- and four-way merges, `sort`
-- through `(>)`, `sortBy cmp` through `cmp x y == GT`), so every
-- comparator that is no total order yields GHC's permutation: the
-- comparator called with the caller's argument order (M7 — the swapped
-- call turned `\_ _ -> GT` into the identity), NaN under `sort` and under
-- `sortBy compare`, which GHC orders differently (N8), constant and
-- never-EQ comparators over inputs that take every path (two, three and
-- four elements; five, six and seven runs; many runs), plus stability,
-- the run shapes (strictly descending, descending with equal neighbours,
-- alternating) and a 10^5-element sort in each direction.
dv :: Number -> Number -> Number
dv a b = a / b

alternating :: Int -> [Int]
alternating n = map (\i -> if even i then i else n - i) [1 .. n]

scrambled :: Int -> Int -> [Int]
scrambled n m = map (\i -> (i * 7919) `mod` m) [1 .. n]

main :: IO ()
main = do
    print (sortBy (\a b -> if fst a <= fst b then LT else GT) [(1, "a"), (0, "z"), (1, "b"), (1, "c")])
    print (sortBy (\_ _ -> GT) [1, 2, 3 :: Int])
    print (sortBy (\_ _ -> LT) [1, 2, 3 :: Int])
    print (sortBy (\_ _ -> EQ) [3, 1, 2 :: Int])
    print (sortBy (\_ _ -> GT) [1 .. 9 :: Int])
    print (sortBy (\_ _ -> GT) [1 .. 24 :: Int])
    print (sortBy (\_ _ -> LT) [1 .. 24 :: Int])
    print (sortBy (\a b -> compare (snd a) (snd b)) [(1, 2), (2, 1), (3, 2), (4, 1 :: Int)] :: [(Int, Int)])
    print (sortBy (\a b -> compare b a) [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5 :: Int])
    let nan = dv 0.0 0.0
    print (sort [2.0, nan, 1.0])
    print (sort [nan, 3.0, 1.0, 2.0])
    print (sort [3.0, 1.0, nan, 2.0, nan, 0.5])
    print (sort [3.0, 0.0, 0.0, 0.0, 1.0, nan, 0.0])
    print (sort [nan, 6.0, 1.0, 4.0, 4.0])
    print (sort [1.0, nan, 2.0, 4.0, nan, 6.0, 0.0, 5.0, nan, nan, 4.0])
    print (sort [0.0, 1.0, 1.0, 3.0, nan, 6.0, 6.0, 2.0, 2.0, 4.0, 0.0])
    print (sort [3.0, 0.0, nan])
    print (sort [4.0, 6.0, 5.0, 5.0, 3.0, 4.0, nan, nan, 1.0, 3.0, 6.0, 1.0, 4.0])
    print (sort [6.0, 4.0, 3.0, 5.0, 1.0, nan])
    print (sort [nan, 0.0, 4.0, nan])
    print (sort [nan, 2.0, 2.0, 5.0, 4.0, 5.0])
    print (sort [3.0, 3.0, nan, 0.0, nan, 3.0, nan, 0.0, nan, 4.0, nan, 0.0])
    print (sort [0.0, nan, 3.0, 2.0, nan, nan, 2.0, 5.0, nan, nan, 4.0])
    print (sort [nan, 0.0, 2.0, 6.0, nan, 4.0, 1.0])
    print (sort [4.0, 4.0, 1.0, 6.0, 6.0, nan, nan])
    print (sort [1.0, nan, 5.0, nan, nan, 2.0, nan, 2.0])
    print (sort [5.0, 2.0, nan, nan, 2.0, nan, 4.0, 3.0, 2.0])
    print (sort [nan, 3.0, 6.0, nan, 3.0, 2.0, nan, 5.0, 3.0, 0.0, 1.0, 0.0])
    print (sort [nan, 0.0, nan, 5.0, nan, 1.0, 6.0])
    print (sort [1.0, nan, nan, 6.0, 2.0])
    print (sort [1.0, nan, 3.0, 6.0, 3.0, 4.0, nan, nan, 6.0, 1.0])
    print (sort [nan, nan, nan, 1.0, nan, 0.0, nan, nan, 2.0, nan, nan, nan, 3.0, nan, nan, nan, 4.0, nan, nan, nan, 5.0, nan])
    print (sortBy compare [2.0, nan, 1.0])
    print (sortBy compare [nan, 3.0, 1.0, 2.0])
    print (sortBy compare [3.0, 1.0, nan, 2.0, nan, 0.5])
    print (sortBy compare [3.0, 0.0, 0.0, 0.0, 1.0, nan, 0.0])
    print (sortBy compare [1.0, nan, 2.0, 4.0, nan, 6.0, 0.0, 5.0, nan, nan, 4.0])
    print (sortBy compare [nan, 3.0, 6.0, nan, 3.0, 2.0, nan, 5.0, 3.0, 0.0, 1.0, 0.0])
    print (sortBy (\a b -> compare b a) [3.0, 1.0, nan, 2.0, nan, 0.5])
    print (sortBy (\a b -> if a < b then GT else LT) [0, 2, 2, 2, 7, 9, 1, 8, 0, 5, 8, 8 :: Int])
    print (sortBy (\a b -> compare (a `mod` 4) (b `mod` 4)) [0, 2, 2, 2, 7, 9, 1, 8, 0, 5, 8, 8 :: Int])
    print (sortBy (\a b -> if a == b then GT else compare a b) [0, 2, 2, 2, 7, 9, 1, 8, 0, 5, 8, 8 :: Int])
    print (sortBy (\a b -> if a < b then GT else LT) [7, 1, 8, 0, 3, 3, 4, 0, 1, 8, 7 :: Int])
    print (sortBy (\a b -> if a == b then GT else compare a b) [7, 1, 8, 0, 3, 3, 4, 0, 1, 8, 7 :: Int])
    print (sortBy (\a b -> if a < b then GT else LT) [6, 7, 5, 1 :: Int])
    print (sortBy (\a b -> if a == b then GT else compare a b) [6, 1, 3, 4, 1, 2 :: Int])
    print (sortBy (\a b -> if a < b then GT else LT) [3, 2, 6, 8, 6 :: Int])
    print (sortBy (\a b -> if a < b then GT else LT) [1, 2 :: Int])
    print (sortBy (\a b -> if a < b then GT else LT) [2, 2, 1 :: Int])
    print (sortBy (\a b -> if a == b then GT else compare a b) [2, 2, 1, 1 :: Int])
    print (sortBy (\a b -> if a < b then GT else LT) (alternating 12))
    print (sortBy (\a b -> if a == b then GT else compare a b) (alternating 14))
    print (sortBy (\a b -> if a < b then GT else LT) (alternating 20))
    print (sortBy (\a b -> if a == b then GT else compare a b) (alternating 24))
    print (sortBy (\a b -> if a < b then GT else LT) (alternating 26))
    print (sortBy (\a b -> if a == b then GT else compare a b) (scrambled 40 17))
    print (sortBy (\a b -> if a < b then GT else LT) (scrambled 50 13))
    print (sortBy (\a b -> compare (a `mod` 5) (b `mod` 5)) (scrambled 30 23))
    print (sort [5, 3, 9, 1, 3, 7, 2, 8, 6, 4 :: Int])
    print (sort [7, 6, 5, 4, 3, 2, 1 :: Int], sort [1, 2, 3, 2, 1 :: Int])
    print (sort ([] :: [Int]), sort [1 :: Int], sort [2, 1 :: Int], sort [2, 3, 1 :: Int], sort [4, 2, 3, 1 :: Int])
    print (sortBy (\a b -> compare (a `mod` 3) (b `mod` 3)) [1 .. 10 :: Int])
    print (sortBy (\a b -> compare (fst a) (fst b)) [(2, "b"), (1, "x"), (2, "a"), (1, "y"), (2, "c"), (0, "q"), (2, "d")])
    print (sortBy (\a b -> compare (fst a) (fst b)) [(3, 1), (3, 2), (2, 3), (2, 4), (1, 5), (1, 6), (0, 7), (0, 8) :: (Int, Int)])
    print (sortBy (\a b -> compare (fst a) (fst b)) [(1, 1), (0, 2), (1, 3), (0, 4), (1, 5), (0, 6), (1, 7), (0, 8), (1, 9) :: (Int, Int)])
    print (sort [9, 8, 7, 6, 5, 4, 3, 2, 1, 0 :: Int])
    print (sort [9, 9, 8, 8, 7, 7, 6, 6, 5, 5 :: Int])
    print (sort [1, 9, 2, 8, 3, 7, 4, 6, 5 :: Int])
    print (sort (alternating 25))
    print (sort (scrambled 60 61))
    print (sort ["pear", "apple", "fig", "apple", "kiwi", ""])
    print (sort [Just 3, Nothing, Just 1, Nothing, Just 2 :: Maybe Int])
    print (sort [(2, 1), (1, 9), (1, 2), (2, 0) :: (Int, Int)])
    print (length (sort [100000, 99999 .. 1 :: Int]), length (sort [1 .. 100000 :: Int]))
    print (take 5 (sort (scrambled 100000 100003)), length (sort (alternating 100000)))
