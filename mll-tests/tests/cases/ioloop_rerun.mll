import Data.IORef
import Control.Monad

countdown :: Int -> IO ()
countdown 0 = putStrLn "done"
countdown n = do
    print n
    countdown (n - 1)

bump :: IORef Int -> Int -> IO ()
bump r 0 = modifyIORef' r (\a -> a + 100)
bump r n = do
    modifyIORef' r (\a -> a + 1)
    bump r (n - 1)

sumDown :: IORef Int -> Int -> Int -> IO Int
sumDown r 0 acc = pure acc
sumDown r n acc = do
    modifyIORef' r (\a -> a + 1)
    sumDown r (n - 1) (acc + n)

work :: IORef Int -> IORef [Int] -> Int -> IO ()
work _ seen 0 = modifyIORef seen (\s -> 0 : s)
work fails seen n = do
    modifyIORef seen (\s -> n : s)
    f <- readIORef fails
    if n == 2 && f > 0
        then writeIORef fails (f - 1) >> error "transient"
        else work fails seen (n - 1)

retry :: Int -> IO () -> IO ()
retry k act = catch act (\_ -> if k > 0 then retry (k - 1) act else putStrLn "gave up")

twice :: IO () -> IO ()
twice a = do
    a
    a

times :: Int -> IO () -> IO ()
times 0 _ = pure ()
times k a = do
    a
    times (k - 1) a

walk :: IORef (IO ()) -> IORef Int -> IORef [Int] -> Int -> IO ()
walk _ _ seen 0 = modifyIORef seen (\s -> 0 : s)
walk slot depth seen n = do
    modifyIORef seen (\s -> n : s)
    d <- readIORef depth
    if n == 2 && d == 0
        then do
            writeIORef depth 1
            inner <- readIORef slot
            inner
        else pure ()
    walk slot depth seen (n - 1)

fill :: STArray s -> Int -> ST s ()
fill arr 0 = pure ()
fill arr n = do
    modifySTArray arr (n - 1) (\v -> v + n)
    fill arr (n - 1)

twiceST :: ST s () -> ST s ()
twiceST a = do
    a
    a

timesST :: Int -> ST s () -> ST s ()
timesST 0 _ = pure ()
timesST k a = do
    a
    timesST (k - 1) a

filled :: Int -> [Int]
filled k = runST (do
    arr <- newSTArray 3 0
    let a = fill arr 3
    timesST k a
    stArrayToList arr)

main :: IO ()
main = do
    twice (countdown 2)
    let c = countdown 1
    c
    c
    r <- newIORef 0
    slot <- newIORef (bump r 3)
    a <- readIORef slot
    a
    a
    v1 <- readIORef r
    print v1
    assert (v1 == 206) "an IORef-stored loop runs whole twice"
    writeIORef r 0
    let b = bump r 2
    sequence_ [b, b, b]
    mapM_ id [b, b]
    v2 <- readIORef r
    print v2
    assert (v2 == 510) "a list of one loop value runs whole each time"
    writeIORef r 0
    when True b
    unless True b
    when False b
    v3 <- readIORef r
    print v3
    assert (v3 == 102) "when runs the loop once, from its start"
    writeIORef r 0
    let z = bump r 5
    times 0 z
    v4 <- readIORef r
    print v4
    assert (v4 == 0) "a loop value run zero times performs nothing"
    times 3 z
    v5 <- readIORef r
    print v5
    assert (v5 == 315) "a loop value run three times"
    writeIORef r 0
    let s = sumDown r 4 0
    x1 <- s
    x2 <- s
    print (x1, x2)
    assert (x1 == 10 && x2 == 10) "the accumulator restarts on every run"
    v6 <- readIORef r
    assert (v6 == 8) "both runs counted every step"
    fails <- newIORef (1 :: Int)
    seen <- newIORef []
    retry 2 (work fails seen 3)
    trace1 <- readIORef seen
    print (reverse trace1)
    assert (reverse trace1 == [3, 2, 3, 2, 1, 0]) "a retry after a raise mid-loop restarts the loop"
    slot2 <- newIORef (pure ())
    depth <- newIORef 0
    seen2 <- newIORef []
    let w = walk slot2 depth seen2 3
    writeIORef slot2 w
    w
    trace2 <- readIORef seen2
    print (reverse trace2)
    assert (reverse trace2 == [3, 2, 3, 2, 1, 0, 1, 0]) "a loop re-entered from its own run starts whole"
    writeIORef r 0
    let big = bump r 1000000
    twice big
    v7 <- readIORef r
    print v7
    assert (v7 == 2000200) "deep loop runs twice in constant stack"
    writeIORef r 0
    times 3 (times 2 (bump r 2))
    v8 <- readIORef r
    print v8
    assert (v8 == 612) "nested loop values"
    print (filled 0)
    print (filled 1)
    print (filled 2)
    assert (filled 2 == [2, 4, 6]) "an ST loop value run twice under runST"
    let st = runST (do
            arr <- newSTArray 3 0
            twiceST (fill arr 3)
            stArrayToList arr)
    print st
    assert (st == [2, 4, 6]) "an ST loop value passed to a combinator"
