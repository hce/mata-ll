import Data.IORef

reseed :: Int -> LuaIO "math.randomseed" ()
draw :: Int -> Int -> LuaIO "math.random" Int
drawUnit :: LuaIO "math.random" Number
drawTry :: Int -> Int -> LuaTry "math.random" (Either String Int)

twice :: IO () -> IO ()
twice a = a >> a

record :: IORef [Int] -> IO Int -> IO ()
record r a = do
    v <- a
    modifyIORef r (v :)

recordTry :: IORef [Int] -> IO (Either String Int) -> IO ()
recordTry r a = do
    e <- a
    case e of
        Left _ -> modifyIORef r (0 :)
        Right v -> modifyIORef r (v :)

recordUnit :: IORef [Number] -> IO Number -> IO ()
recordUnit r a = do
    v <- a
    modifyIORef r (v :)

direct :: Int -> IO [Int]
direct k = do
    reseed 7
    xs <- mapM (\_ -> draw 1 1000000000) [1 .. k]
    pure (reverse xs)

main :: IO ()
main = do
    expected <- direct 4
    reseed 7
    r <- newIORef []
    twice (record r (draw 1 1000000000))
    let pending = draw 1 1000000000
    pending `seq` record r pending
    record r pending
    got <- readIORef r
    assert (got == expected) "an FFI action passed as a value is performed at each run and not when evaluated"
    reseed 7
    t <- newIORef []
    twice (recordTry t (drawTry 1 1000000000))
    let unrun = drawTry 1 1000000000
    unrun `seq` recordTry t unrun
    gotTry <- readIORef t
    assert (gotTry == drop 1 expected) "a LuaTry action passed as a value is performed at each run"
    reseed 7
    s <- newIORef []
    let acts = map (draw 1) [1000000000, 1000000000]
    mapM_ (record s) acts
    mapM_ (record s) acts
    gotList <- readIORef s
    assert (gotList == expected) "first-class FFI actions stored in a list are performed at each run"
    reseed 7
    u <- newIORef []
    twice (recordUnit u drawUnit)
    us <- readIORef u
    reseed 7
    a1 <- drawUnit
    a2 <- drawUnit
    assert (us == [a2, a1]) "a nullary FFI action is performed at each run"
    putStrLn "ffi action values ok"
