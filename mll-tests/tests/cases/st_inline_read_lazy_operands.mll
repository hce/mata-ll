-- A bind-chain `v <- readSTArray arr i` is spelled inline at its site
-- (`local v = arr[i + 1]`): the emitted code indexes the array and adds to
-- the index itself, so both operands must be delivered in WHNF there. A
-- lazy index (a loop's suspended `i + 1`, a lazy let, a lazy parameter, a
-- record field, a list element) raised "attempt to perform arithmetic on
-- a table value"; a lazy array handle (an if, a case, a lazy let, a
-- function returning one of two handles, a lazy parameter) was indexed as
-- the thunk table and silently read its slot. The writes, modifies and
-- the IORef operations take their operands through runtime functions
-- that force them; pinned here with the same lazy operands, next to the
-- concrete twin of the loop (every operand proven WHNF, read bare).
module Main where
import Data.IORef

data Ix = Ix { ixAt :: Int, ixOff :: Int }

lazyInt :: Int -> Int
lazyInt n = length (replicate n ())

sumArr :: STArray s -> Int -> Int -> ST s Int
sumArr arr i acc = do
    n <- stArrayLength arr
    if i >= n then pure acc else do
        v <- readSTArray arr i
        sumArr arr (i + 1) (acc + v)

sumStrict :: STArray s -> Int -> Int -> ST s Int
sumStrict arr i acc = arr `seq` i `seq` acc `seq` do
    n <- stArrayLength arr
    if i >= n then pure acc else do
        v <- readSTArray arr i
        sumStrict arr (i + 1) (acc + v)

pick :: Bool -> STArray s -> STArray s -> STArray s
pick b x y = if b then x else y

pickRef :: Bool -> IORef Int -> IORef Int -> IORef Int
pickRef b x y = if b then x else y

readAt :: STArray s -> Int -> ST s Int
readAt arr k = do
    n <- stArrayLength arr
    if n == 0 then pure (-1) else readSTArray arr k

readH :: Bool -> STArray s -> ST s Int
readH b arr = if b then pure 0 else do
    v <- readSTArray arr 1
    pure (v + 1)

writeH :: Bool -> STArray s -> Int -> ST s ()
writeH b arr k = if b then pure () else do
    writeSTArray arr k 99
    modifySTArray arr k (+ 1)

readAll :: STArray s -> [Int] -> ST s [Int]
readAll arr [] = pure []
readAll arr (k : ks) = do
    v <- readSTArray arr k
    vs <- readAll arr ks
    pure (v : vs)

bump :: Bool -> IORef Int -> IO Int
bump b r = if b then pure 0 else do
    writeIORef r 5
    modifyIORef r (+ 1)
    modifyIORef' r (* 2)
    readIORef r

main :: IO ()
main = do
    print (runST (do
        arr <- newSTArrayFromList [1, 2, 3]
        sumArr arr 0 0))
    print (runST (do
        arr <- newSTArrayFromList [1, 2, 3]
        sumStrict arr 0 0))
    print (runST (do
        a <- newSTArray 3 7
        let arr = pick (lazyInt 1 > 5) a a
        v <- readSTArray arr 1
        pure (v == 7)))
    print (runST (do
        arr <- newSTArrayFromList [10, 20, 30, 40]
        let j = lazyInt 2
        v <- readSTArray arr j
        pure v))
    print (runST (do
        arr <- newSTArrayFromList [10, 20, 30, 40]
        readAt arr (lazyInt 3)))
    print (runST (do
        arr <- newSTArrayFromList [10, 20, 30, 40]
        let ix = Ix { ixAt = lazyInt 1, ixOff = 0 }
        v <- readSTArray arr (ixAt ix)
        pure v))
    print (runST (do
        arr <- newSTArrayFromList [10, 20, 30, 40]
        let ks = [lazyInt 3, 0]
        v <- readSTArray arr (head ks)
        pure v))
    print (runST (do
        arr <- newSTArrayFromList [10, 20, 30, 40]
        readAll arr [lazyInt 1, lazyInt 2, 0]))
    print (runST (do
        a <- newSTArrayFromList [10, 20, 30, 40]
        b <- newSTArrayFromList [5, 6]
        let arrIf = if lazyInt 1 > 5 then a else b
        let arrCase = case lazyInt 2 of
                2 -> b
                _ -> a
        let arrLet = pick True b a
        let arrFn = pick (lazyInt 1 > 5) a b
        p <- readSTArray arrIf 0
        q <- readSTArray arrCase 1
        r <- readSTArray arrLet 0
        s <- readSTArray arrFn 1
        t <- readH (lazyInt 1 > 5) (pick (lazyInt 2 > 5) a b)
        pure [p, q, r, s, t]))
    print (runST (do
        a <- newSTArrayFromList [10, 20, 30, 40]
        let arr = pick (lazyInt 1 > 5) a a
        writeSTArray arr (lazyInt 1) 99
        modifySTArray arr (lazyInt 2) (* 2)
        writeH (lazyInt 1 > 5) (pick (lazyInt 2 > 5) a a) (lazyInt 3)
        x <- readSTArray a 1
        y <- readSTArray a 2
        z <- readSTArray a 3
        pure (x, y, z)))
    r1 <- newIORef 1
    r2 <- newIORef 2
    let r = pickRef (lazyInt 1 > 5) r1 r2
    writeIORef r 5
    modifyIORef r (+ 1)
    modifyIORef' r (* 2)
    v <- readIORef r
    w <- readIORef r1
    u <- bump (lazyInt 1 > 5) (pickRef (lazyInt 2 > 5) r1 r2)
    print (v, w, u)
