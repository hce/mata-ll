-- Conway's Game of Life, sparse representation: the world is the list of
-- live cells. No fixed grid is stored; only the viewport is bounded, at
-- render time. Relies on tuple equality; rows are built with mconcat over
-- String literals (String is opaque, so `++`/`map` over characters do not
-- apply).
module Main where

import Data.List (nubBy)

type Cell = (Int, Int)

width :: Int
width = 20

height :: Int
height = 12

-- The eight Moore neighbours of a cell.
neighbours :: Cell -> [Cell]
neighbours (x, y) =
  [ (x + dx, y + dy)
  | dx <- [-1, 0, 1]
  , dy <- [-1, 0, 1]
  , not (dx == 0 && dy == 0)
  ]

same :: Cell -> Cell -> Bool
same (a, b) (c, d) = a == c && b == d

member :: Cell -> [Cell] -> Bool
member c cs = any (same c) cs

nub :: [Cell] -> [Cell]
nub = nubBy same

-- One generation. A live cell survives with 2 or 3 live neighbours; a dead
-- cell is born with exactly 3. Only cells that are live or adjacent to a
-- live cell are tested -- every other cell has zero neighbours, stays dead.
step :: [Cell] -> [Cell]
step live = [ c | c <- candidates, born c ]
  where
    candidates = nub (live ++ concatMap neighbours live)
    liveNeighbours c = length (filter (\p -> member p live) (neighbours c))
    born c = liveNeighbours c == 3 || (liveNeighbours c == 2 && member c live)

renderRow :: [Cell] -> Int -> String
renderRow live y = mconcat [ if member (x, y) live then "#" else "." | x <- [0 .. width - 1] ]

draw :: [Cell] -> IO ()
draw live = mapM_ (\y -> putStrLn (renderRow live y)) [0 .. height - 1]

run :: Int -> [Cell] -> IO ()
run 0 _ = return ()
run n live = do
  draw live
  putStrLn (mconcat (replicate width "-"))
  run (n - 1) (step live)

-- A glider.
glider :: [Cell]
glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)]

main :: IO ()
main = run 6 glider
