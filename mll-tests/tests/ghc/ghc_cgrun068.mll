-- GHC cgrun068: Conway's Game of Life step function

nth :: Integer -> [a] -> a
nth 0 (x:_) = x
nth n (_:xs) = nth (n - 1) xs
nth _ [] = error "index"

getCell :: [[Integer]] -> Integer -> Integer -> Integer
getCell grid r c =
    let rows = length grid
        cols = if rows == 0 then 0 else length (head grid)
    in if r < 0 || r >= rows || c < 0 || c >= cols then 0 else nth c (nth r grid)

neighbours :: [[Integer]] -> Integer -> Integer -> Integer
neighbours grid r c =
    foldl (+) 0 [getCell grid (r + dr) (c + dc) | dr <- [-1, 0, 1], dc <- [-1, 0, 1], not (dr == 0 && dc == 0)]

nextCell :: [[Integer]] -> Integer -> Integer -> Integer
nextCell grid r c =
    let n = neighbours grid r c
        alive = getCell grid r c == 1
    in if alive then (if n == 2 || n == 3 then 1 else 0) else (if n == 3 then 1 else 0)

step :: [[Integer]] -> [[Integer]]
step grid =
    let rows = length grid
        cols = if rows == 0 then 0 else length (head grid)
    in [[nextCell grid r c | c <- [0..(cols-1)]] | r <- [0..(rows-1)]]

main :: IO ()
main = do
    let blinker = [[0,1,0],[0,1,0],[0,1,0]]
    let after1  = [[0,0,0],[1,1,1],[0,0,0]]
    assert (step blinker == after1) "blinker step 1"
    assert (step after1 == blinker) "blinker step 2"

    let block = [[0,0,0,0],[0,1,1,0],[0,1,1,0],[0,0,0,0]]
    assert (step block == block) "block still life"

    let dead = [[0,0,0],[0,0,0],[0,0,0]]
    assert (step dead == dead) "dead stays dead"

    let single = [[0,0,0],[0,1,0],[0,0,0]]
    assert (step single == dead) "lone cell dies"
    putStrLn "ok"
