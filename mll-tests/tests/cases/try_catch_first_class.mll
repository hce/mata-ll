import Data.IORef

data Guarded = Guarded { name :: String, run :: IO Int }

risky :: Int -> IO Int
risky n = if n > 2 then error ("too big " <> show n) else pure (n * 10)

noisy :: IORef [String] -> String -> IO Int -> IO Int
noisy log tag a = do
    modifyIORef log (tag :)
    a

safely :: IO Int -> IO Int
safely a = a `catch` \m -> putStrLn ("safely: " <> m) >> pure (-1)

attempt :: IO a -> IO (Either String a)
attempt = try

orZero :: IO Int -> IO Int
orZero = flip catch (\_ -> pure 0)

infixl 1 `orElse`
orElse :: IO Int -> (String -> IO Int) -> IO Int
orElse = catch

(?>) :: IO Int -> (String -> IO Int) -> IO Int
a ?> h = catch a h

guardOnce :: IO Int -> IO Int
guardOnce act = act `seq` catch act (\_ -> pure 0)

showEither :: Either String Int -> String
showEither (Left m) = "Left " <> m
showEither (Right v) = "Right " <> show v

main :: IO ()
main = do
    a <- try $ putStrLn "effect"
    case a of
        Left m -> putStrLn ("prefix-dollar left: " <> m)
        Right _ -> putStrLn "prefix-dollar right"
    b <- risky 5 `catch` (\_ -> pure 0)
    print b
    c <- risky 1 `catch` (\_ -> pure 0)
    print c
    d <- catch (pure 1) $ \_ -> pure (3 :: Int)
    print d
    e <- catch (risky 4) $ \m -> putStrLn ("dollar handler: " <> m) >> pure 7
    print e
    f <- (`catch` (\_ -> pure 11)) (risky 6)
    print f
    g <- (risky 2 `catch`) (\_ -> pure 12)
    print g
    let handleWith = catch (risky 8)
    h <- handleWith (\_ -> pure 13)
    print h
    rs <- mapM try [pure (1 :: Int), error "x", risky 3, risky 2]
    putStrLn (show (map showEither rs))
    ts <- traverse (try . risky) [1, 2, 3]
    putStrLn (show (map showEither ts))
    us <- sequence (map (`catch` (\_ -> pure (-5))) [risky 0, risky 9])
    print us
    i <- (try . risky) 7
    putStrLn (showEither i)
    let tryRisky = try . risky
    j <- tryRisky 1
    putStrLn (showEither j)
    let guards = [Guarded "low" (safely (risky 1)), Guarded "high" (safely (risky 3))]
    vs <- mapM run guards
    print vs
    vs2 <- mapM run guards
    print vs2
    w1 <- risky 24 `orElse` (\_ -> pure 24)
    w2 <- error "raised computing the action" ?> (\_ -> pure 25)
    print (w1, w2)
    k <- attempt (risky 10)
    putStrLn (showEither k)
    l <- orZero (risky 11)
    print l
    let stored = (try (risky 12), catch (risky 1) (\_ -> pure 0))
    m1 <- fst stored
    putStrLn (showEither m1)
    m2 <- snd stored
    print m2
    n <- catch (catch (risky 13) (\m -> error ("re: " <> m))) (\m -> putStrLn ("outer: " <> m) >> pure 14)
    print n
    o <- try (try (risky 15))
    case o of
        Right (Left m) -> putStrLn ("inner left: " <> m)
        Right (Right _) -> putStrLn "inner right"
        Left _ -> putStrLn "outer left"
    p <- try (risky 1 `catch` (\_ -> risky 16))
    putStrLn (showEither p)
    q <- try (risky 17 `catch` (\_ -> risky 18))
    putStrLn (showEither q)
    calls <- newIORef (0 :: Int)
    r <- catch (pure 19) (\_ -> modifyIORef calls (+ 1) >> pure 0)
    r2 <- risky 1 `catch` (\_ -> modifyIORef calls (+ 1) >> pure 0)
    ncalls <- readIORef calls
    print (r, r2, ncalls)
    log <- newIORef []
    s <- try (noisy log "first" (noisy log "second" (risky 20)))
    putStrLn (showEither s)
    entries <- readIORef log
    print entries
    cell <- newIORef (0 :: Int)
    t <- try (do
        writeIORef cell 1
        _ <- risky 21
        writeIORef cell 2)
    cellv <- readIORef cell
    print cellv
    case t of
        Left msg -> putStrLn ("effects kept, then " <> msg)
        Right _ -> putStrLn "no raise"
    lazyv <- try (pure (error "never forced" :: Int))
    case lazyv of
        Left _ -> putStrLn "lazy caught"
        Right _ -> putStrLn "lazy right"
    forcedv <- try (let z = error "forced" :: Int in z `seq` pure z)
    putStrLn (showEither forcedv)
    outside <- try (guardOnce (error "computed outside"))
    putStrLn (showEither outside)
    inside <- try (catch (error "computed inside" :: IO Int) (\m -> putStrLn ("handler saw: " <> m) >> pure 22))
    putStrLn (showEither inside)
    twice <- newIORef (0 :: Int)
    let bumpThenFail = modifyIORef twice (+ 1) >> risky 23
    let guarded = try bumpThenFail
    _ <- guarded
    _ <- guarded
    tv <- readIORef twice
    print tv
    assert (tv == 2) "a stored try action runs its action on every run"
