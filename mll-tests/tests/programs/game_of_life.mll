-- Conway's Game of Life on an unbounded grid held as a set of live
-- coordinates: neighbour counting through a tuple-keyed map, generation
-- stepping, period detection, and rendering of a window.
-- Leans on: Data.Set and Data.Map with (Int, Int) keys, tuple
-- comprehensions, foldr over set contents, iterate/takeWhile over
-- generations, Bool-driven string rendering.
import Data.Map (Map)
import qualified Data.Map as M
import qualified Data.Set as S

type Cell = (Int, Int)
type World = S.Set Cell

neighbourCounts :: World -> Map Cell Int
neighbourCounts world =
    foldr (\c m -> M.insertWith (+) c 1 m) M.empty
        [ (x + dx, y + dy)
        | (x, y) <- S.toList world
        , dx <- [-1, 0, 1]
        , dy <- [-1, 0, 1]
        , (dx, dy) /= (0, 0) ]

step :: World -> World
step world = S.fromList
    [ c
    | (c, n) <- M.toList (neighbourCounts world)
    , n == 3 || (n == 2 && S.member c world) ]

render :: Int -> Int -> Int -> Int -> World -> String
render x0 y0 x1 y1 world =
    mconcat [ mconcat [if S.member (x, y) world then "#" else "." | x <- [x0 .. x1]] <> "\n" | y <- [y0 .. y1] ]

fromBools :: [[Bool]] -> World
fromBools rows = S.fromList [(x, y) | (y, row) <- zip [0 ..] rows, (x, alive) <- zip [0 ..] row, alive]

glider :: World
glider = fromBools
    [ [False, True, False]
    , [False, False, True]
    , [True, True, True] ]

blinker :: World
blinker = fromBools [[True, True, True]]

rpentomino :: World
rpentomino = fromBools
    [ [False, True, True]
    , [True, True, False]
    , [False, True, False] ]

block :: World
block = fromBools [[True, True], [True, True]]

-- Generations until the world repeats, if it does within the bound.
period :: Int -> World -> Maybe Int
period bound world = go 1 (step world)
  where
    go n w
        | n > bound = Nothing
        | S.toList w == S.toList world = Just n
        | otherwise = go (n + 1) (step w)

bounds :: World -> (Int, Int, Int, Int)
bounds world =
    let xs = map fst (S.toList world)
        ys = map snd (S.toList world)
    in (minimum xs, minimum ys, maximum xs, maximum ys)

showGen :: (Int, World) -> IO ()
showGen (i, w) = do
    putStrLn ("generation " <> show i <> " population " <> show (S.size w))
    putStr (render 0 0 5 5 w)

main :: IO ()
main = do
    putStr (render 0 0 5 5 glider)
    putStrLn "--"
    let gens = iterate step glider
    mapM_ showGen (zip [1 .. 4 :: Int] (tail gens))
    print (bounds (gens !! 8), bounds (gens !! 40))
    print (S.toList (gens !! 4))
    print (period 10 blinker, period 10 block, period 10 glider, period 3 rpentomino)
    print (map (\n -> S.size (iterate step rpentomino !! n)) [0, 10, 20, 30, 40, 50])
    print (bounds (iterate step rpentomino !! 50))
    print (M.toList (neighbourCounts block))
    print (S.size (step S.empty), S.null (step (S.singleton (0, 0))))
    print (S.member (1, 0) glider, S.member (0, 0) glider, S.toList (S.difference glider (step glider)))
