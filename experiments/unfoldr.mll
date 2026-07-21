unfoldr :: (s -> Maybe (a, s)) -> s -> [a]
unfoldr gen state =
    case gen state of
        Nothing          -> []
        Just (a, state') -> a:unfoldr gen state'

rangor :: Integer -> Integer -> [Integer]
rangor from to =
    unfoldr (\s -> if s > to then Nothing else Just (s, s + 1)) from

main :: IO ()
main = do
    putStrLn "Let us together count from one to 12, okay?"
    mapM_ (\num -> putStrLn $ "And the next number is " <> show num) $ rangor 1 12
