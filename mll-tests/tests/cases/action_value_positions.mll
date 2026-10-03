import Data.IORef

data Box = Box (IO ())

data Handler = Handler { label :: String, act :: IO () }

class Greeter a where
    greet :: a -> IO ()

data En = En

instance Greeter En where
    greet _ = putStrLn "hello"

twice :: IO () -> IO ()
twice a = a >> a

swap :: IO () -> IO () -> IO ()
swap a b = b >> a

never :: IO () -> IO ()
never _ = pure ()

repeatN :: Int -> IO () -> IO ()
repeatN 0 _ = pure ()
repeatN k a = a >> repeatN (k - 1) a

single :: Int -> IO ()
single n = putStrLn ("single " <> show n)

guarded :: Int -> IO ()
guarded n
    | n > 100 = putStrLn "big"
    | otherwise = putStrLn ("guarded " <> show n)

multi :: Int -> IO ()
multi 0 = putStrLn "multi zero"
multi n = putStrLn ("multi " <> show n)

tagged :: String -> Int -> IO ()
tagged s n = do
    putStr s
    print n

bump :: IORef Int -> Int -> IO Int
bump r d = do
    modifyIORef' r (+ d)
    readIORef r

collect :: Int -> IO Int -> IO [Int]
collect 0 _ = pure []
collect k a = do
    x <- a
    rest <- collect (k - 1) a
    pure (x : rest)

runBox :: Box -> IO ()
runBox (Box a) = a >> a

pick :: Bool -> IO () -> IO () -> IO ()
pick c a b = if c then a else b

stTwice :: ST s () -> ST s ()
stTwice a = a >> a

addAt :: STArray s -> Int -> ST s ()
addAt arr i = modifySTArray arr i (\v -> v + 1)

fromWhere :: IO ()
fromWhere = twice early
  where
    early = single late
    late = length [(), ()] + 40

main :: IO ()
main = do
    let t = length [(), (), ()]
    putStrLn "-- arguments"
    twice (print t)
    twice (single t)
    twice (guarded t)
    twice (multi t)
    twice (tagged "tagged " t)
    let p = tagged "partial "
    twice (p t)
    let lam = \x -> putStrLn ("lambda " <> show (x + t))
    twice (lam t)
    twice (local t)
    twice (greet En)
    twice (putStrLn $ "dollar " <> show t)
    twice (when (t > 1) (print (t * 10)))
    twice (mapM_ print [t, t + 1])
    putStrLn "-- order"
    swap (print t) (print (t + 1))
    swap (single t) (multi t)
    putStrLn "-- zero runs"
    never (print t)
    never (single undefined)
    single undefined `seq` putStrLn "seq of an unrun action"
    print t `seq` putStrLn "seq done"
    _ <- pure $! print t
    case print t of
        _ -> putStrLn "case done"
    pick False (print t) (putStrLn "picked")
    putStrLn "-- repeats"
    repeatN 3 (print t)
    sequence_ (replicate 2 (single t))
    putStrLn "-- structures"
    let acts = map print [t, t + 1]
    sequence_ acts
    sequence_ acts
    let lit = [single t, multi t]
    sequence_ lit
    mapM_ id lit
    let zs = zipWith (\a b -> print (a + b)) [1, 2 :: Int] [t, t]
    sequence_ zs
    sequence_ zs
    let cs = [print (x * t) | x <- [1, 2 :: Int]]
    sequence_ cs
    sequence_ cs
    let pair = (print t, t)
    fst pair
    fst pair
    runBox (Box (single t))
    let h = Handler { label = "h", act = print (t + 2) }
    act h
    act h
    let m = Just (multi t)
    case m of
        Just a -> a >> a
        Nothing -> pure ()
    let comp = map (print . (+ 1)) [t]
    sequence_ comp
    sequence_ comp
    putStrLn "-- bindings"
    fromWhere
    let early = single later
        later = t + 50
    early
    early
    let shared = single (length [t, t, t, t])
    twice shared
    putStrLn "-- pure payloads and references"
    a <- pure (print t)
    a
    a
    r <- newIORef (pure ())
    writeIORef r (single (t + 4))
    b <- readIORef r
    b
    b
    c <- newIORef (0 :: Int)
    xs <- collect 3 (bump c (t + 1))
    print xs
    ys <- sequence (replicate 2 (bump c 10))
    print ys
    putStrLn "-- ST"
    let s = runST (do
            arr <- newSTArray 2 (0 :: Int)
            stTwice (addAt arr 0)
            stTwice (modifySTArray arr 1 (\v -> v + 5))
            x <- readSTArray arr 0
            y <- readSTArray arr 1
            pure (x, y))
    print s
  where
    local x = putStrLn ("local " <> show x)
