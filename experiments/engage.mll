export processEvent :: forall s. (Integer -> Integer -> LuaIO s Integer) -> Integer -> LuaIO s Integer
processEvent f n = do
    (liftIO . putStrLn) $ "Called from Lua with " <> show n
    f n (n + 1)

main :: IO ()
main = putStrLn "engage test ok"
