-- GHC cgrun052: Matrix as list of lists (transpose, element-wise operations)

transpose_ :: [[Integer]] -> [[Integer]]
transpose_ [] = []
transpose_ (r:_) = if length r == 0 then [] else transposeGo (r : tail (r : []))
  where
    transposeGo _ = []

-- Simple transpose via index
transposeM :: [[Integer]] -> [[Integer]]
transposeM rows = case rows of
    [] -> []
    (r:_) -> [getCol j rows | j <- [0..(length r - 1)]]

getCol :: Integer -> [[Integer]] -> [Integer]
getCol _ [] = []
getCol j (row:rows) = nth j row : getCol j rows

nth :: Integer -> [a] -> a
nth 0 (x:_) = x
nth n (_:xs) = nth (n - 1) xs
nth _ [] = error "index out of bounds"

matAdd :: [[Integer]] -> [[Integer]] -> [[Integer]]
matAdd a b = zipWith (zipWith (+)) a b

matScale :: Integer -> [[Integer]] -> [[Integer]]
matScale k m = map (map (* k)) m

dot :: [Integer] -> [Integer] -> Integer
dot xs ys = foldl (+) 0 (zipWith (*) xs ys)

matMul :: [[Integer]] -> [[Integer]] -> [[Integer]]
matMul a b = let bt = transposeM b in map (\row -> map (dot row) bt) a

identity :: Integer -> [[Integer]]
identity n = [[if i == j then 1 else 0 | j <- [0..(n-1)]] | i <- [0..(n-1)]]

main :: IO ()
main = do
    let m = [[1,2,3],[4,5,6],[7,8,9]]
    assert (transposeM m == [[1,4,7],[2,5,8],[3,6,9]]) "transpose 3x3"
    assert (transposeM (transposeM m) == m) "double transpose"

    let a = [[1,0],[0,1]]
    let b = [[2,3],[4,5]]
    assert (matMul a b == b) "identity * b"
    assert (matMul b a == b) "b * identity"

    let c = [[1,2],[3,4]]
    let d = [[5,6],[7,8]]
    assert (matMul c d == [[19,22],[43,50]]) "2x2 multiply"
    assert (matAdd c d == [[6,8],[10,12]]) "matAdd"
    assert (matScale 3 c == [[3,6],[9,12]]) "matScale"
    assert (identity 3 == [[1,0,0],[0,1,0],[0,0,1]]) "identity 3x3"
    putStrLn "ok"
