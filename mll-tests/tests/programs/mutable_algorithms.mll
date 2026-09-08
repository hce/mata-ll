-- Mutation in IO and ST: IORef accumulators and counters driven by
-- forM_/when/unless, a sieve of Eratosthenes and prefix sums on an
-- STArray inside runST, an in-place bubble sort, and a Fisher-Yates
-- shuffle with a seeded generator held in an IORef.
-- Leans on: Data.IORef (newIORef/readIORef/writeIORef/modifyIORef'),
-- runST with newSTArray/readSTArray/writeSTArray/modifySTArray/
-- stArrayToList/newSTArrayFromList/stArrayLength, Control.Monad loops,
-- IORefs holding lists and maps, sequencing of effects with results.
import Data.IORef
import Control.Monad (forM_, when, unless, forM)
import Data.Map (Map)
import qualified Data.Map as M

sieve :: Int -> [Int]
sieve n = runST (do
    arr <- newSTArray (n + 1) 1
    writeSTArray arr 0 0
    writeSTArray arr 1 0
    forM_ [2 .. n] (\i -> do
        isPrime <- readSTArray arr i
        when (isPrime == 1 && i * i <= n) (forM_ [i * i, i * i + i .. n] (\j -> writeSTArray arr j 0)))
    flags <- stArrayToList arr
    pure [i | (i, f) <- zip [0 ..] flags, f == 1])

prefixSums :: [Int] -> [Int]
prefixSums xs = runST (do
    arr <- newSTArrayFromList xs
    n <- stArrayLength arr
    forM_ [1 .. n - 1] (\i -> do
        prev <- readSTArray arr (i - 1)
        modifySTArray arr i (+ prev))
    stArrayToList arr)

bubbleSort :: [Int] -> [Int]
bubbleSort xs = runST (do
    arr <- newSTArrayFromList xs
    n <- stArrayLength arr
    forM_ [0 .. n - 2] (\pass ->
        forM_ [0 .. n - 2 - pass] (\i -> do
            a <- readSTArray arr i
            b <- readSTArray arr (i + 1)
            when (a > b) (do
                writeSTArray arr i b
                writeSTArray arr (i + 1) a)))
    stArrayToList arr)

histogram :: [Int] -> Int -> [Int]
histogram xs buckets = runST (do
    arr <- newSTArray buckets 0
    forM_ xs (\x -> modifySTArray arr (x `mod` buckets) (+ 1))
    stArrayToList arr)

nextRandom :: IORef Int -> IO Int
nextRandom ref = do
    s <- readIORef ref
    let s' = (s * 75 + 74) `mod` 65537
    writeIORef ref s'
    pure s'

shuffle :: IORef Int -> [Int] -> IO [Int]
shuffle gen xs = go xs []
  where
    go [] acc = pure acc
    go remaining acc = do
        r <- nextRandom gen
        let i = r `mod` length remaining
            picked = remaining !! i
        go (take i remaining ++ drop (i + 1) remaining) (picked : acc)

main :: IO ()
main = do
    print (sieve 100)
    print (length (sieve 2000), sieve 1, sieve 2)
    print (prefixSums [1 .. 10], prefixSums [], prefixSums [5])
    print (bubbleSort [5, 2, 9, 1, 5, 6, -3, 0], bubbleSort [], bubbleSort [1])
    print (histogram [1 .. 100] 7, histogram [] 3)
    counter <- newIORef (0 :: Int)
    forM_ [1 .. 10] (\i -> do
        when (even i) (modifyIORef' counter (+ i))
        unless (even i) (modifyIORef' counter (subtract 1)))
    readIORef counter >>= print
    total <- newIORef (0 :: Integer)
    log' <- newIORef []
    forM_ [1 .. 20] (\i -> do
        modifyIORef' total (+ toInteger (i * i))
        t <- readIORef total
        when (t `mod` 7 == 0) (modifyIORef log' ((i, t) :)))
    readIORef total >>= print
    readIORef log' >>= print . reverse
    table <- newIORef (M.empty :: Map String Int)
    forM_ ["a", "b", "a", "c", "b", "a"] (\k -> modifyIORef' table (M.insertWith (+) k 1))
    readIORef table >>= print . M.toList
    gen <- newIORef 12345
    shuffled <- shuffle gen [1 .. 10]
    print shuffled
    print (bubbleSort shuffled == [1 .. 10])
    draws <- forM [1 .. 5 :: Int] (\_ -> nextRandom gen)
    print draws
    seeds <- mapM (\s -> do
        ref <- newIORef s
        a <- nextRandom ref
        b <- nextRandom ref
        pure (a, b)) [1, 2, 3]
    print seeds
    flag <- newIORef False
    writeIORef flag True
    readIORef flag >>= print
    lazyRef <- newIORef (error "never read" :: Int)
    writeIORef lazyRef 42
    readIORef lazyRef >>= print
