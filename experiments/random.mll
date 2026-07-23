rr :: LuaIO "math.random" Number
rr2 :: Int -> Int -> LuaIO "math.random" Int

main :: IO ()
main = do
    randNum <- rr
    putStrLn $ "A number between 0.0 and 1.0: " <> show randNum
    randNum <- rr
    putStrLn $ "A number between 0.0 and 1.0: " <> show randNum
    randNum2 <- rr2 23 42
    putStrLn $ "An integer between 23 and 42: " <> show randNum2
