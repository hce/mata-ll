export processEvent :: forall s. (Int -> Int -> LuaIO s Int) -> Int -> LuaIO s Int
processEvent f n = do
    (liftIO . putStrLn) $ "Called from Lua with " <> show n
    f n (n + 1)

main :: IO ()
main = putStrLn "engage test ok"
