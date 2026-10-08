classify :: Int -> String
classify n = go n
  where
    go x | x > 10 = "big"
    go 0 = "zero"
    go _ = "other"

firstBig :: [Int] -> String
firstBig xs = go xs
  where
    go (x:_) | x > 100 = "huge head"
    go (x:rest) | x > 10 = "big head, " <> go rest
    go [] = "end"
    go _ = "small head"

shape :: Maybe Int -> String
shape m = go m
  where
    go (Just x) | even x = "even"
                | x > 100 = "large odd"
    go (Just _) = "odd"
    go Nothing = "none"

bounded :: Int -> Int -> Int
bounded lo x = clamp x
  where
    clamp y | y < lo = lo
            | otherwise = y

lookupName :: Int -> String
lookupName k = pick k
  where
    pick n | Just s <- lookup n table = s
    pick 0 = "root"
    pick _ = "unknown"
    table = [(1, "one"), (2, "two")]

main :: IO ()
main = do
    mapM_ (putStrLn . classify) [11, 0, 5]
    putStrLn (firstBig [200])
    putStrLn (firstBig [20, 30, 1])
    putStrLn (firstBig [20, 30])
    putStrLn (firstBig [])
    putStrLn (firstBig [3])
    mapM_ (putStrLn . shape) [Just 4, Just 101, Just 3, Nothing]
    print (bounded 5 2, bounded 5 8)
    mapM_ (putStrLn . lookupName) [1, 2, 0, 9]
    let grade s | s >= 90 = "A"
        grade s | s >= 80 = "B"
        grade _ = "C"
    mapM_ (putStrLn . grade) [95, 85, 10 :: Int]
    let step 0 = "done"
        step n | odd n = "odd " <> rest
               | n > 4 = "big even " <> rest
          where rest = step (n - 1)
        step n = "small even " <> step (n - 1)
    putStrLn (step (6 :: Int))
    putStrLn (let pick x | x > 0 = "pos"
                  pick 0 = "zero"
                  pick _ = "neg"
              in mconcat (map pick [1, 0, -1 :: Int]))
