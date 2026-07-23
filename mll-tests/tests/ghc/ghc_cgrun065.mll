-- GHC cgrun065: Frequency counting

data FreqEntry = FreqEntry Int Int
    deriving (Show, Eq)

freqValue :: FreqEntry -> Int
freqValue (FreqEntry x _) = x

freqCount :: FreqEntry -> Int
freqCount (FreqEntry _ n) = n

-- Count occurrences of each element
frequencies :: [Int] -> [FreqEntry]
frequencies [] = []
frequencies (x:xs) =
    let cnt = 1 + length (filter (== x) xs)
        rest = frequencies (filter (/= x) xs)
    in FreqEntry x cnt : rest

-- Sort by frequency descending (insertion sort)
sortByFreqDesc :: [FreqEntry] -> [FreqEntry]
sortByFreqDesc [] = []
sortByFreqDesc (e:es) = insertByFreq e (sortByFreqDesc es)

insertByFreq :: FreqEntry -> [FreqEntry] -> [FreqEntry]
insertByFreq e [] = [e]
insertByFreq e (f:fs)
    | freqCount e >= freqCount f = e : f : fs
    | otherwise                  = f : insertByFreq e fs

mostCommon :: [Int] -> Maybe Int
mostCommon [] = Nothing
mostCommon xs = Just (freqValue (head (sortByFreqDesc (frequencies xs))))

main :: IO ()
main = do
    let xs = [1,2,3,2,1,2,3,1,2]
    let freq = frequencies xs
    assert (mostCommon xs == Just 2) "most common is 2"
    assert (mostCommon [1] == Just 1) "single element"
    assert (mostCommon [] == Nothing) "empty list"

    let sorted = sortByFreqDesc freq
    assert (freqCount (head sorted) == 4) "highest freq is 4"

    assert (length freq == 3) "3 distinct elements"
    -- total counts
    let total = foldl (\acc e -> acc + freqCount e) 0 freq
    assert (total == length xs) "total counts"
    putStrLn "ok"
