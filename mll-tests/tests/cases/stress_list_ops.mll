-- Stress test: large list operations

myTake :: Integer -> [a] -> [a]
myTake 0 _ = []
myTake _ [] = []
myTake n (x:xs) = x : myTake (n - 1) xs

myDrop :: Integer -> [a] -> [a]
myDrop 0 xs = xs
myDrop _ [] = []
myDrop n (_:xs) = myDrop (n - 1) xs

myReplicate :: Integer -> a -> [a]
myReplicate 0 _ = []
myReplicate n x = x : myReplicate (n - 1) x

mySum :: [Integer] -> Integer
mySum xs = foldl (+) 0 xs

myProduct :: [Integer] -> Integer
myProduct xs = foldl (*) 1 xs

range :: Integer -> Integer -> [Integer]
range a b = if a > b then [] else a : range (a + 1) b

qsort :: [Integer] -> [Integer]
qsort [] = []
qsort (p:xs) = appendList (appendList (qsort lesser) (p : equal)) (qsort greater)
  where
    lesser = filter (< p) xs
    equal = filter (== p) xs
    greater = filter (> p) xs

appendList :: [a] -> [a] -> [a]
appendList [] ys = ys
appendList (x:xs) ys = x : appendList xs ys

isSorted :: [Integer] -> Bool
isSorted [] = True
isSorted (_:[]) = True
isSorted (a:b:rest) = a <= b && isSorted (b : rest)

myZipWith :: (a -> b -> c) -> [a] -> [b] -> [c]
myZipWith _ [] _ = []
myZipWith _ _ [] = []
myZipWith f (a:as_) (b:bs) = f a b : myZipWith f as_ bs

isEven :: Integer -> Bool
isEven x = x `mod` 2 == 0

main :: IO ()
main = do
    let xs = range 1 1000
    assert (length xs == 1000) "range 1000"
    assert (mySum xs == 500500) "sum 1..1000"
    assert (length (myTake 100 xs) == 100) "take 100"
    assert (length (myDrop 900 xs) == 100) "drop 900"
    let reps = myReplicate 500 7
    assert (length reps == 500) "replicate 500"
    assert (mySum reps == 3500) "sum replicated"
    let rev50 = reverse (range 1 50)
    let sorted = qsort rev50
    assert (isSorted sorted) "qsort sorted"
    assert (length sorted == 50) "qsort length"
    let zipped = myZipWith (+) (range 1 200) (range 1 200)
    assert (length zipped == 200) "zipWith length"
    assert (mySum zipped == 40200) "zipWith sum"
    let evens = filter isEven (range 1 500)
    assert (length evens == 250) "filter evens"
    let mapped = map (\x -> x * x) (range 1 100)
    assert (length mapped == 100) "map squares"
    putStrLn "ok"
