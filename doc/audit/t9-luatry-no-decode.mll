module Main where
tryList :: Int -> LuaTry "try_list" (Either String [Int])
export run :: IO ()
run = do
  r <- tryList 0
  case r of
    Right xs -> print (sum xs)
    Left e   -> putStrLn ("err: " <> e)
