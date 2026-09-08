-- Sudoku: a backtracking solver that always fills the cell with the fewest
-- candidates, over a list-of-lists grid, producing a lazy stream of
-- solutions.
-- Leans on: list comprehensions, laziness (only the first solution of the
-- stream is ever forced), nested list indexing, Data.List sorting,
-- where-bound helpers, string rendering with mconcat.
import Data.List (sortBy)

type Grid = [[Int]]

transpose' :: [[a]] -> [[a]]
transpose' [] = []
transpose' ([] : _) = []
transpose' xss = map head xss : transpose' (map tail xss)

boxes :: Grid -> [[Int]]
boxes g =
    [ concat [take 3 (drop c row) | row <- take 3 (drop r g)]
    | r <- [0, 3, 6], c <- [0, 3, 6] ]

boxIndex :: Int -> Int -> Int
boxIndex r c = 3 * (r `div` 3) + c `div` 3

candidates :: Grid -> (Int, Int) -> [Int]
candidates g (r, c) =
    [v | v <- [1 .. 9], not (elem v used)]
  where
    used = (g !! r) ++ (transpose' g !! c) ++ (boxes g !! boxIndex r c)

emptyCells :: Grid -> [(Int, Int)]
emptyCells g = [(r, c) | (r, row) <- zip [0 ..] g, (c, v) <- zip [0 ..] row, v == 0]

set :: Grid -> (Int, Int) -> Int -> Grid
set g (r, c) v =
    [ if ri == r then [if ci == c then v else x | (ci, x) <- zip [0 ..] row] else row
    | (ri, row) <- zip [0 ..] g ]

-- Every solution, most constrained cell first; a cell with no candidate
-- prunes the branch because concatMap over [] yields nothing.
solve :: Grid -> [Grid]
solve g = case emptyCells g of
    [] -> [g]
    cells ->
        let ranked = sortBy (\(_, a) (_, b) -> compare (length a) (length b))
                        [(cell, candidates g cell) | cell <- cells]
            (cell, cands) = head ranked
        in concatMap (\v -> solve (set g cell v)) cands

noDuplicates :: [Int] -> Bool
noDuplicates xs = let filled = filter (/= 0) xs in length filled == length (nub' filled)
  where
    nub' [] = []
    nub' (y : ys) = y : nub' (filter (/= y) ys)

valid :: Grid -> Bool
valid g = all noDuplicates g && all noDuplicates (transpose' g) && all noDuplicates (boxes g)

render :: Grid -> String
render g = mconcat (map (\row -> renderRow row <> "\n") g)
  where renderRow row = mconcat (map (\v -> if v == 0 then ". " else show v <> " ") row)

puzzle :: Grid
puzzle =
    [ [5, 3, 0, 0, 7, 0, 0, 0, 0]
    , [6, 0, 0, 1, 9, 5, 0, 0, 0]
    , [0, 9, 8, 0, 0, 0, 0, 6, 0]
    , [8, 0, 0, 0, 6, 0, 0, 0, 3]
    , [4, 0, 0, 8, 0, 3, 0, 0, 1]
    , [7, 0, 0, 0, 2, 0, 0, 0, 6]
    , [0, 6, 0, 0, 0, 0, 2, 8, 0]
    , [0, 0, 0, 4, 1, 9, 0, 0, 5]
    , [0, 0, 0, 0, 8, 0, 0, 7, 9]
    ]

main :: IO ()
main = do
    putStr (render puzzle)
    putStrLn ("empty cells: " <> show (length (emptyCells puzzle)))
    putStrLn ("valid start: " <> show (valid puzzle))
    let solutions = solve puzzle
    case take 1 solutions of
        [] -> putStrLn "no solution"
        (s : _) -> do
            putStrLn ""
            putStr (render s)
            putStrLn ("valid: " <> show (valid s) <> ", complete: " <> show (null (emptyCells s)))
            -- Blank the last row of the solution: the columns and boxes
            -- still force it back, so exactly one solution remains.
            let reopened = take 8 s ++ [replicate 9 0]
            putStrLn ("solutions after reopening a row: " <> show (length (solve reopened)))
    -- A contradictory grid (two 5s in the first row) has no solutions.
    let broken = set puzzle (0, 1) 5
    putStrLn ("broken is valid: " <> show (valid broken))
    putStrLn ("broken solutions: " <> show (null (solve broken)))
    putStrLn ("candidates at (0,2): " <> show (candidates puzzle (0, 2)))
    putStrLn ("candidates at (4,4): " <> show (candidates puzzle (4, 4)))
