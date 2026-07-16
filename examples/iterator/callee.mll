iterateMe :: LuaIterator "iterateme" [Integer]

iterateMe2 :: String -> String -> LuaIterator "string.gmatch" [String]

export run :: IO ()
run = do
    print $ take 20 iterateMe
    print $ take 20 $ iterateMe2 "Foo Foo Foo" "Foo"
