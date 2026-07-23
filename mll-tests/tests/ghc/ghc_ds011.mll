-- GHC ds011: List comprehension with pattern matching generators

listIndex :: [a] -> Int -> a
listIndex (x:_)  0 = x
listIndex (_:xs) n = listIndex xs (n - 1)
listIndex []     _ = error "index out of bounds"

data Shape = Circle Number | Rect Number Number
    deriving (Show, Eq)

area :: Shape -> Number
area (Circle r)  = 3.14159 * r * r
area (Rect w h)  = w * h

-- Pattern match in generator: only Just values
fromJusts :: [Maybe Int] -> [Int]
fromJusts ms = [x | Just x <- ms]

-- Pattern match on pairs
swapPairs :: [(Int, Int)] -> [(Int, Int)]
swapPairs ps = [(b, a) | (a, b) <- ps]

-- Nested comprehension
matrix :: [[Int]]
matrix = [[i * j | j <- [1..4]] | i <- [1..3]]

-- Comprehension with multiple guards
tripleFilter :: [Int] -> [Int] -> [(Int, Int)]
tripleFilter xs ys = [(x, y) | x <- xs, y <- ys, x + y > 5, x /= y]

-- Comprehension using pattern and guard together
positiveAreas :: [Shape] -> [Number]
positiveAreas shapes = [area s | s <- shapes, area s > 10.0]

main :: IO ()
main = do
    -- fromJusts via pattern generator
    let ms = [Just 1, Nothing, Just 2, Nothing, Just 3]
    assert (fromJusts ms == [1, 2, 3]) "fromJusts"
    assert (fromJusts [Nothing, Nothing] == ([] :: [Int])) "fromJusts empty"

    -- pair pattern in generator
    assert (swapPairs [(1,2),(3,4)] == [(2,1),(4,3)]) "swapPairs"

    -- nested comprehension
    assert (listIndex matrix 0 == [1,2,3,4]) "matrix row 0"
    assert (listIndex matrix 1 == [2,4,6,8]) "matrix row 1"
    assert (listIndex matrix 2 == [3,6,9,12]) "matrix row 2"

    -- multiple guards
    let tf = tripleFilter [1,2,3,4] [2,3,4]
    -- Check membership manually using list comparison
    assert (filter (\p -> fst p == 1 && snd p == 5) tf == []) "tf no 1 5"
    assert (filter (\p -> fst p == 3 && snd p == 4) tf /= []) "tf has 3 4"
    assert (filter (\p -> fst p == 2 && snd p == 4) tf /= []) "tf has 2 4"
    assert (filter (\p -> fst p == 3 && snd p == 3) tf == []) "tf no equal"

    -- positiveAreas
    let shapes = [Circle 2.0, Rect 1.0 1.0, Rect 4.0 3.0, Circle 5.0]
    let pas = positiveAreas shapes
    assert (length pas == 3) "positiveAreas count"

    putStrLn "ok"
