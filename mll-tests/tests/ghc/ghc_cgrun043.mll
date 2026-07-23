-- GHC cgrun043: Do-notation and IO sequencing
-- Tests monadic IO operations in do blocks

printN :: Int -> Int -> IO ()
printN n max
    | n > max   = pure ()
    | otherwise = do
        putStr (show n)
        putStr " "
        printN (n + 1) max

countdown :: Int -> [Int]
countdown 0 = [0]
countdown n = n : countdown (n - 1)

main :: IO ()
main = do
    -- Basic sequencing
    putStr "a"
    putStr "b"
    putStr "c"
    putStrLn ""

    -- Recursive IO
    printN 1 5
    putStrLn ""

    -- let in do
    let xs = [1, 2, 3, 4, 5]
    let total = foldl (+) 0 xs
    assert (total == 15) "let in do"

    -- mapM_ for side effects
    mapM_ (\x -> putStr (show x <> " ")) [10, 20, 30]
    putStrLn ""

    -- when conditional
    when True (putStr "yes ")
    when False (putStr "no ")
    putStrLn ""

    putStrLn "ok"
