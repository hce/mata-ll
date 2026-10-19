refuted :: IO Int
refuted = do
    Just x <- return (Nothing :: Maybe Int)
    return x

matched :: IO Int
matched = do
    Just x <- return (Just 41)
    return (x + 1)

explicit :: IO Int
explicit = fail "boom"

lazyAction :: IO Int
lazyAction = do
    let a = explicit
    putStrLn "built, not run"
    a

report :: Either String Int -> IO ()
report r = case r of
    Left _ -> putStrLn "caught"
    Right v -> print v

main :: IO ()
main = do
    r1 <- try refuted
    report r1
    r2 <- try matched
    report r2
    r3 <- try explicit
    case r3 of
        Left msg -> putStrLn msg
        Right v -> print v
    r4 <- try lazyAction
    case r4 of
        Left msg -> putStrLn msg
        Right v -> print v
    r5 <- catch refuted (\_ -> return 7)
    print r5
    putStrLn "done"
