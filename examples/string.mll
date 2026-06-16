gmatch :: String -> String -> LuaIterator "string.gmatch" String
gmatchPairs :: String -> String -> LuaIterator "string.gmatch" (String, String)

main :: IO ()
main = do
    -- Simple word splitting
    let words = gmatch "Mata lai le kiao" "%w+"
    mapM_ (\w -> putStrLn w) words

    -- Capture groups as tuples
    let pairs = gmatchPairs "name=Hans lang=MLL" "(%w+)=(%w+)"
    mapM_ (\p -> putStrLn (fst p <> " -> " <> snd p)) pairs
