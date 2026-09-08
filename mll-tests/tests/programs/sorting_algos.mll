-- Sorting: merge sort, quicksort, insertion sort, selection sort and a
-- bottom-up merge, checked against the library sort, plus stability
-- checks of sortBy on keyed records and a tour of Data.List.
-- Leans on: Ord-polymorphic top-level functions used at several types,
-- laziness (take from a lazily merged sort), Data.List (partition,
-- groupBy, nubBy, intersperse, intercalate, unfoldr, scanl, scanr,
-- find, foldl'), tuple and record comparisons.
import Data.List (sortBy, partition, groupBy, nubBy, intersperse, intercalate, unfoldr, scanl, scanr, find, foldl')

mergeSort :: Ord a => [a] -> [a]
mergeSort [] = []
mergeSort [x] = [x]
mergeSort xs = merge (mergeSort left) (mergeSort right)
  where
    (left, right) = halve xs
    halve ys = (take (length ys `div` 2) ys, drop (length ys `div` 2) ys)

merge :: Ord a => [a] -> [a] -> [a]
merge [] ys = ys
merge xs [] = xs
merge (x : xs) (y : ys)
    | x <= y = x : merge xs (y : ys)
    | otherwise = y : merge (x : xs) ys

quickSort :: Ord a => [a] -> [a]
quickSort [] = []
quickSort (p : xs) = quickSort smaller ++ [p] ++ quickSort larger
  where (smaller, larger) = partition (< p) xs

insertionSort :: Ord a => [a] -> [a]
insertionSort = foldr insert []
  where
    insert x [] = [x]
    insert x (y : ys)
        | x <= y = x : y : ys
        | otherwise = y : insert x ys

selectionSort :: Ord a => [a] -> [a]
selectionSort [] = []
selectionSort xs = m : selectionSort (removeFirst m xs)
  where
    m = minimum xs
    removeFirst _ [] = []
    removeFirst v (y : ys) = if y == v then ys else y : removeFirst v ys

bottomUp :: Ord a => [a] -> [a]
bottomUp xs = case rounds (map (\x -> [x]) xs) of
    [] -> []
    (r : _) -> r
  where
    rounds [] = []
    rounds [r] = [r]
    rounds rs = rounds (pairUp rs)
    pairUp (a : b : rest) = merge a b : pairUp rest
    pairUp rest = rest

isSorted :: Ord a => [a] -> Bool
isSorted xs = and (zipWith (<=) xs (drop 1 xs))

data Entry = Entry { key :: Int, tag :: String }
    deriving (Show, Eq)

entries :: [Entry]
entries = [Entry 3 "c1", Entry 1 "a1", Entry 2 "b1", Entry 3 "c2", Entry 1 "a2", Entry 2 "b2", Entry 3 "c3"]

-- A pseudo-random permutation from a linear congruential generator.
pseudoRandom :: Int -> Int -> [Int]
pseudoRandom seed n = take n (tail (iterate (\s -> (s * 75 + 74) `mod` 65537) seed))

main :: IO ()
main = do
    let xs = map (`mod` 1000) (pseudoRandom 7 200)
        expected = sort xs
    print (take 12 xs)
    print (mergeSort xs == expected, quickSort xs == expected, insertionSort xs == expected, selectionSort xs == expected, bottomUp xs == expected)
    print (isSorted expected, isSorted xs, isSorted ([] :: [Int]))
    print (take 5 (mergeSort xs), take 5 (quickSort xs))
    print (mergeSort ["banana", "apple", "cherry", "apple"], quickSort [(2, "b"), (1, "z"), (1, "a")])
    print (mergeSort [3.5, -1.25, 2.0 :: Number], insertionSort [True, False, True])
    print (sortBy (\a b -> compare (key a) (key b)) entries)
    print (map tag (sortBy (\a b -> compare (key b) (key a)) entries))
    print (map (map tag) (groupBy (\a b -> key a == key b) (sortBy (\a b -> compare (key a) (key b)) entries)))
    print (nubBy (\a b -> key a == key b) entries)
    print (find (\e -> key e == 2) entries, find (\e -> key e == 9) entries)
    print (partition even [1 .. 10 :: Int], intersperse 0 [1, 2, 3 :: Int], intercalate [0] [[1], [2, 3], [4 :: Int]])
    print (unfoldr (\n -> if n > 100 then Nothing else Just (n, n * 2)) 1)
    print (scanl (+) 0 [1 .. 10 :: Int], scanr (+) 0 [1 .. 5 :: Int], scanl (flip (:)) [] [1, 2, 3 :: Int])
    print (foldl' (+) 0 [1 .. 100000 :: Int], foldl' max 0 xs, foldr (\x acc -> acc + x) 0 [1 .. 10 :: Int])
    print (sortBy (\a b -> compare (snd a) (snd b)) (zip [1 .. 6 :: Int] [2, 1, 2, 1, 3, 1 :: Int]))
    print (sort [[3, 1], [2, 9, 9], [2, 9], [], [3]], sort [Just 3, Nothing, Just 1], sort [Right 1, Left "b", Left "a", Right 0 :: Either String Int])
    print (maximum (map length [[1, 2], [1, 2, 3], []]), minimum ["pear", "apple", "fig"])
