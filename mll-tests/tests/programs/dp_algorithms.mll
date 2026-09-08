-- Dynamic programming with lazy tables: longest common subsequence with
-- reconstruction, edit distance, coin-change counting, 0/1 knapsack, and
-- longest increasing subsequence, each table a list of lists whose rows
-- refer to earlier rows.
-- Leans on: self-referential lazy lists as memo tables, zipWith3-free
-- row recurrences, Integer counts, tuple-carrying rows, string
-- reconstruction through LString.
import LString

-- LCS over code lists; table row i column j is the LCS length of the
-- first i and first j elements. Each row is computed from the previous
-- row and from itself (the cell to the left).
lcsRow :: [Int] -> Int -> [Int] -> [Int]
lcsRow ys x prev = go prev 0 ys
  where
    go (diag : above : rest) left (y : ys') =
        let cell = if x == y then diag + 1 else max above left
        in left : go (above : rest) cell ys'
    go _ left _ = [left]

lcsTable :: [Int] -> [Int] -> [[Int]]
lcsTable xs ys = table
  where table = replicate (length ys + 1) 0 : zipWith (lcsRow ys) xs table

lcsLength :: String -> String -> Int
lcsLength a b = last (last (lcsTable (strToInts a) (strToInts b)))

-- Reconstruct one LCS by walking the table back from the corner.
lcs :: String -> String -> String
lcs a b = mconcat (map strChar (reverse (walk (length xs) (length ys))))
  where
    xs = strToInts a
    ys = strToInts b
    table = lcsTable xs ys
    at i j = table !! i !! j
    walk 0 _ = []
    walk _ 0 = []
    walk i j
        | xs !! (i - 1) == ys !! (j - 1) = xs !! (i - 1) : walk (i - 1) (j - 1)
        | at (i - 1) j >= at i (j - 1) = walk (i - 1) j
        | otherwise = walk i (j - 1)

editRow :: [Int] -> (Int, Int) -> [Int] -> [Int]
editRow ys (i, x) prev = go prev i ys
  where
    go (diag : above : rest) left (y : ys') =
        let cell = minimum [above + 1, left + 1, diag + (if x == y then 0 else 1)]
        in left : go (above : rest) cell ys'
    go _ left _ = [left]

editDistance :: String -> String -> Int
editDistance a b = last (last table)
  where
    xs = strToInts a
    ys = strToInts b
    table = [0 .. length ys] : zipWith (editRow ys) (zip [1 ..] xs) table

-- Ways to make each amount from the coins, one lazy row per coin.
coinChange :: [Int] -> Int -> Integer
coinChange coins target = last (foldl addCoin (1 : replicate target 0) coins)
  where
    addCoin ways coin =
        let next = zipWith (+) ways (replicate coin 0 ++ next)
        in next

knapsack :: Int -> [(String, Int, Int)] -> (Int, [String])
knapsack capacity items = best !! capacity
  where
    best = foldl addItem (replicate (capacity + 1) (0, [])) items
    addItem row (label, weight, value) =
        [ if w >= weight && fst (row !! (w - weight)) + value > fst (row !! w)
            then (fst (row !! (w - weight)) + value, label : snd (row !! (w - weight)))
            else row !! w
        | w <- [0 .. capacity] ]

-- Longest increasing subsequence, quadratic, with the sequence itself.
lis :: [Int] -> [Int]
lis xs = reverse (longest (foldl extend [] xs))
  where
    extend acc x =
        let candidates = [x : s | s <- acc, head s < x]
            bestPrefix = longest candidates
        in acc ++ [if null bestPrefix then [x] else bestPrefix]
    longest = foldl (\b s -> if length s > length b then s else b) []

main :: IO ()
main = do
    print (lcsLength "AGGTAB" "GXTXAYB", lcs "AGGTAB" "GXTXAYB")
    print (lcsLength "kitten" "sitting", lcs "kitten" "sitting")
    print (lcs "abc" "", lcs "" "abc", lcs "abc" "abc")
    print (map (\(a, b) -> editDistance a b) [("kitten", "sitting"), ("flaw", "lawn"), ("", "abc"), ("same", "same"), ("intention", "execution")])
    print (coinChange [1, 2, 5] 5, coinChange [1, 5, 10, 25, 50] 100, coinChange [2] 3, coinChange [3, 7] 0)
    print (coinChange [1, 2, 5, 10, 20, 50, 100, 200] 200)
    print (knapsack 10 [("gold", 4, 40), ("silver", 3, 25), ("bronze", 5, 30), ("gem", 2, 45), ("iron", 6, 20)])
    print (knapsack 0 [("gold", 4, 40)], knapsack 3 [])
    print (lis [10, 9, 2, 5, 3, 7, 101, 18], lis [0, 8, 4, 12, 2, 10, 6, 14, 1, 9], lis [], lis [5, 4, 3])
