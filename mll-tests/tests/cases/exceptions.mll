-- Exception handling: try and catch for IO errors

main :: IO ()
main = do
    -- Test 1: try succeeds → Right
    result1 <- try (pure (42 :: Integer))
    case result1 of
        Right v -> assert (v == 42) "try pure gives Right"
        Left _  -> error "should not be Left"

    -- Test 2: try catches error → Left
    result2 <- try (error "boom" :: IO Integer)
    case result2 of
        Right _ -> error "should not be Right"
        Left _  -> putStrLn "try error gives Left"

    -- Test 3: try with IO action that succeeds
    result3 <- try (putStrLn "hello from try")
    case result3 of
        Right _ -> putStrLn "try putStrLn succeeded"
        Left _  -> error "putStrLn should not fail"

    -- Test 4: catch with no error → runs action
    v4 <- catch (pure (10 :: Integer)) (\_ -> pure 0)
    assert (v4 == 10) "catch no error"

    -- Test 5: catch with error → runs handler
    v5 <- catch (error "oops" :: IO Integer) (\_ -> pure 99)
    assert (v5 == 99) "catch with error runs handler"

    -- Test 6: nested try
    result6 <- try (do
        x <- pure (1 :: Integer)
        y <- pure (2 :: Integer)
        pure (x + y))
    case result6 of
        Right v -> assert (v == 3) "nested try"
        Left _  -> error "should not fail"

    -- Test 7: try catches non-exhaustive pattern
    result7 <- try (do
        let xs = [] :: [Integer]
        pure (head xs))
    case result7 of
        Right _ -> error "head [] should fail"
        Left _  -> putStrLn "caught head []"

    putStrLn "ok"
