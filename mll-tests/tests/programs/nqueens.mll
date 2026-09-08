-- N-queens: all solutions by backtracking over columns, solution counts
-- for several board sizes, symmetry classification, and a rendered board.
-- Leans on: list comprehensions with multiple guards, laziness (the
-- first solution of a large board is found without enumerating the
-- rest), Int arithmetic, string rendering, nested folds.
queens :: Int -> [[Int]]
queens n = go n
  where
    go 0 = [[]]
    go k = [q : qs | qs <- go (k - 1), q <- [1 .. n], safe q qs]
    safe q qs = and [q /= c && abs (q - c) /= d | (d, c) <- zip [1 ..] qs]

render :: Int -> [Int] -> String
render n qs = mconcat [row q <> "\n" | q <- qs]
  where row q = mconcat [if c == q then "Q " else ". " | c <- [1 .. n]]

-- The eight symmetries of a solution as placements.
rotate :: Int -> [Int] -> [Int]
rotate n qs = [n + 1 - (position c) | c <- [1 .. n]]
  where position c = head [r | (r, q) <- zip [1 ..] qs, q == c]

mirror :: Int -> [Int] -> [Int]
mirror n qs = map (\q -> n + 1 - q) qs

symmetries :: Int -> [Int] -> [[Int]]
symmetries n qs = rotations ++ map (mirror n) rotations
  where rotations = take 4 (iterate (rotate n) qs)

distinctSolutions :: Int -> Int
distinctSolutions n = length (go (queens n) [])
  where
    go [] seen = seen
    go (s : rest) seen
        | any (\t -> elem t seen) (symmetries n s) = go rest seen
        | otherwise = go rest (s : seen)

main :: IO ()
main = do
    print (map (\n -> length (queens n)) [1 .. 8])
    print (map distinctSolutions [4 .. 7])
    putStr (render 6 (head (queens 6)))
    putStr (render 8 (head (queens 8)))
    print (head (queens 10))
    print (take 3 (queens 7))
    print (length (symmetries 6 (head (queens 6))))
    print (rotate 4 [2, 4, 1, 3], mirror 4 [2, 4, 1, 3])
