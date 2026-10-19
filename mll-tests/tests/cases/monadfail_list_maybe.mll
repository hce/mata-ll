justs :: [Maybe Int] -> [Int]
justs xs = do
    Just x <- xs
    return x

inner :: Maybe (Maybe Int) -> Maybe Int
inner m = do
    Just x <- m
    return (x + 1)

rights :: [Either String Int] -> [Int]
rights es = do
    Right v <- es
    return (v * 10)

heads :: [[Int]] -> [Int]
heads xss = do
    (x : _) <- xss
    return x

zeros :: [Int] -> [String]
zeros ns = do
    0 <- ns
    return "zero"

polyFail :: MonadFail m => m (Maybe Int) -> m Int
polyFail e = do
    Just x <- e
    return x

nested :: [Maybe (Maybe Int)] -> [Int]
nested ms = do
    Just m <- ms
    y <- do
        Just x <- [m, Just 100]
        return x
    return y

explicit :: Maybe Int
explicit = fail "never shown"

main :: IO ()
main = do
    print (justs [Just 1, Nothing, Just 3])
    print (inner (Just Nothing))
    print (inner (Just (Just 4)))
    print (inner Nothing)
    print (rights [Right 1, Left "x", Right 2])
    print (heads [[1, 2], [], [3]])
    print (zeros [0, 1, 0])
    print (polyFail [Just 1, Nothing, Just 2])
    print (polyFail (Just Nothing))
    print (polyFail (Just (Just 9)))
    print (nested [Just (Just 1), Nothing, Just Nothing])
    print explicit
    print (fail "dropped" :: [Int])
    print [x | Just x <- [Just 1, Nothing, Just 3]]
    print [a + b | (a, b) <- [(1, 2), (3, 4)]]
