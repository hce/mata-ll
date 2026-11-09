-- Every STArray access is bounds-checked, as GHC's is: an array of size n
-- has the indices 0 .. n-1, and a read, write or modify of any other index
-- raises `Ix{Int}.index: Index (i) out of range ((0,n-1))` where `catch`
-- and `try` see it. A read past the end used to return Lua's nil typed as
-- an Int and a write past the end stored into the table and left a hole;
-- both were silent. Pinned for every access form: the read spelled inline
-- at a bind site (literal, variable and loop indices), the helper call (an
-- index that is a `div`), an index taken from another read, a discarded
-- and a final read, the write and the modify in statement position, and
-- the first-class actions (a let-bound action or constructor, an action
-- passed to a function, forM_, when). Edges: the last index, the length
-- itself, negative and far indices, an empty array, an array from a list,
-- and a negative size (an empty array that keeps the bounds it was
-- given). The hot loops run 20000 accesses in range before the one out
-- of range, so that a host with a JIT meets it in compiled code.
import Control.Monad (forM_, when)

attempt :: String -> Int -> IO ()
attempt tag v =
    catch (v `seq` putStrLn (tag <> " = " <> show v))
          (\msg -> putStrLn (tag <> " raised " <> msg))

attemptList :: String -> [Int] -> IO ()
attemptList tag xs =
    catch (sum xs `seq` putStrLn (tag <> " = " <> show xs))
          (\msg -> putStrLn (tag <> " raised " <> msg))

sumThrough :: STArray s -> Int -> Int -> Int -> ST s Int
sumThrough arr i lastIx acc =
    if i > lastIx then pure acc else do
        v <- readSTArray arr i
        sumThrough arr (i + 1) lastIx (acc + v)

fillThrough :: STArray s -> Int -> Int -> ST s ()
fillThrough arr i lastIx =
    if i > lastIx then pure () else do
        writeSTArray arr i (i * i)
        fillThrough arr (i + 1) lastIx

bumpThrough :: STArray s -> Int -> Int -> ST s ()
bumpThrough arr i lastIx =
    if i > lastIx then pure () else do
        modifySTArray arr i (+ 1)
        bumpThrough arr (i + 1) lastIx

readHalf :: STArray s -> Int -> ST s Int
readHalf arr k = do
    v <- readSTArray arr (k `div` 2)
    pure (v + 1)

chase :: STArray s -> Int -> ST s Int
chase arr start = do
    next <- readSTArray arr start
    v <- readSTArray arr next
    pure (next * 100 + v)

sumStrict :: STArray s -> Int -> Int -> Int -> ST s Int
sumStrict arr i lastIx acc = acc `seq` (if i > lastIx then pure acc else do
    v <- readSTArray arr i
    sumStrict arr (i + 1) lastIx (acc + v))

twice :: ST s Int -> ST s Int
twice act = do
    a <- act
    b <- act
    pure (a + b)

through :: ST s a -> ST s a
through act = act

main :: IO ()
main = do
    attempt "read first" (runST (do
        arr <- newSTArray 3 7
        readSTArray arr 0))
    attempt "read last" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        v <- readSTArray arr 2
        pure v))
    attempt "read at length" (runST (do
        arr <- newSTArray 3 7
        v <- readSTArray arr 3
        pure v))
    attempt "read past length" (runST (do
        arr <- newSTArray 3 7
        v <- readSTArray arr 4
        pure (v + 1)))
    attempt "read negative" (runST (do
        arr <- newSTArray 3 7
        v <- readSTArray arr (-1)
        pure v))
    attempt "read far" (runST (do
        arr <- newSTArray 3 7
        v <- readSTArray arr 1000000
        pure v))
    attempt "read far negative" (runST (do
        arr <- newSTArray 3 7
        v <- readSTArray arr (-1000000)
        pure v))
    attempt "write last" (runST (do
        arr <- newSTArray 3 7
        writeSTArray arr 2 1
        a <- readSTArray arr 2
        n <- stArrayLength arr
        pure (a * 10 + n)))
    attempt "write at length" (runST (do
        arr <- newSTArray 3 7
        writeSTArray arr 3 1
        readSTArray arr 0))
    attempt "write past length then read and length" (runST (do
        arr <- newSTArray 3 7
        writeSTArray arr 5 1
        x <- readSTArray arr 4
        n <- stArrayLength arr
        pure (if x == 0 then n else n + 100)))
    attempt "write negative" (runST (do
        arr <- newSTArray 3 7
        writeSTArray arr (-1) 1
        readSTArray arr 0))
    attempt "write bottom out of range" (runST (do
        arr <- newSTArray 3 7
        writeSTArray arr 9 undefined
        pure 1))
    attempt "write bottom in range" (runST (do
        arr <- newSTArray 3 7
        writeSTArray arr 1 undefined
        a <- readSTArray arr 0
        c <- readSTArray arr 2
        pure (a + c)))
    attempt "modify last" (runST (do
        arr <- newSTArray 3 7
        modifySTArray arr 2 (* 3)
        readSTArray arr 2))
    attempt "modify at length" (runST (do
        arr <- newSTArray 3 7
        modifySTArray arr 3 (* 3)
        readSTArray arr 0))
    attempt "modify negative" (runST (do
        arr <- newSTArray 3 7
        modifySTArray arr (-2) (* 3)
        readSTArray arr 0))
    attempt "modify out of range leaves the function unapplied" (runST (do
        arr <- newSTArray 3 7
        modifySTArray arr 7 (\_ -> error "the function was applied")
        readSTArray arr 0))
    attempt "modify out of range with a bottom function" (runST (do
        arr <- newSTArray 3 7
        modifySTArray arr 9 undefined
        readSTArray arr 0))
    attempt "bottom index" (runST (do
        arr <- newSTArray 3 7
        writeSTArray arr undefined 1
        readSTArray arr 0))
    attempt "list length" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        stArrayLength arr))
    attempt "list read at length" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        v <- readSTArray arr 3
        pure v))
    attempt "list write at length" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        writeSTArray arr 3 40
        stArrayLength arr))
    attempt "list modify at length" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        modifySTArray arr 3 (+ 1)
        stArrayLength arr))
    attempt "empty list length" (runST (do
        arr <- newSTArrayFromList []
        stArrayLength arr))
    attempt "empty list read" (runST (do
        arr <- newSTArrayFromList []
        v <- readSTArray arr 0
        pure v))
    attempt "empty length" (runST (do
        arr <- newSTArray 0 7
        stArrayLength arr))
    attempt "empty read" (runST (do
        arr <- newSTArray 0 7
        v <- readSTArray arr 0
        pure v))
    attempt "empty write" (runST (do
        arr <- newSTArray 0 7
        writeSTArray arr 0 1
        stArrayLength arr))
    attempt "negative size length" (runST (do
        arr <- newSTArray (-3) 7
        stArrayLength arr))
    attemptList "negative size elements" (runST (do
        arr <- newSTArray (-3) 7
        stArrayToList arr))
    attempt "negative size read" (runST (do
        arr <- newSTArray (-3) 7
        v <- readSTArray arr 0
        pure v))
    attempt "negative size write" (runST (do
        arr <- newSTArray (-3) 7
        writeSTArray arr 0 1
        stArrayLength arr))
    attempt "loop sum in range" (runST (do
        arr <- newSTArrayFromList [1, 2, 3, 4]
        sumThrough arr 0 3 0))
    attempt "loop sum one past" (runST (do
        arr <- newSTArrayFromList [1, 2, 3, 4]
        sumThrough arr 0 4 0))
    attempt "loop fill in range" (runST (do
        arr <- newSTArray 4 0
        fillThrough arr 0 3
        sumThrough arr 0 3 0))
    attempt "loop fill one past" (runST (do
        arr <- newSTArray 4 0
        fillThrough arr 0 4
        sumThrough arr 0 3 0))
    attempt "loop modify in range" (runST (do
        arr <- newSTArray 4 5
        bumpThrough arr 0 3
        sumThrough arr 0 3 0))
    attempt "loop modify one past" (runST (do
        arr <- newSTArray 4 5
        bumpThrough arr 0 4
        sumThrough arr 0 3 0))
    attempt "hot loop in range" (runST (do
        arr <- newSTArray 20000 1
        fillThrough arr 0 9999
        bumpThrough arr 10000 19999
        a <- sumStrict arr 0 9999 0
        b <- sumStrict arr 10000 19999 0
        pure (a `mod` 1000 + b)))
    attempt "hot loop sum one past" (runST (do
        arr <- newSTArray 20000 1
        sumStrict arr 0 20000 0))
    attempt "hot loop fill one past" (runST (do
        arr <- newSTArray 20000 1
        fillThrough arr 0 20000
        sumStrict arr 0 19999 0))
    attempt "hot loop modify one past" (runST (do
        arr <- newSTArray 20000 1
        bumpThrough arr 0 20000
        sumStrict arr 0 19999 0))
    attempt "hot loop sum from before the start" (runST (do
        arr <- newSTArray 20000 1
        a <- sumStrict arr 0 19999 0
        b <- sumStrict arr (-1) 19999 0
        pure (a + b)))
    attempt "computed index in range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        readHalf arr 5))
    attempt "computed index at length" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        readHalf arr 6))
    attempt "computed index negative" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        readHalf arr (-1)))
    attempt "chased index in range" (runST (do
        arr <- newSTArrayFromList [2, 20, 30]
        chase arr 0))
    attempt "chased index out of range" (runST (do
        arr <- newSTArrayFromList [7, 20, 30]
        chase arr 0))
    attempt "chased index negative" (runST (do
        arr <- newSTArrayFromList [-4, 20, 30]
        chase arr 0))
    attempt "discarded read" (runST (do
        arr <- newSTArray 3 7
        _ <- readSTArray arr 9
        pure 1))
    attempt "sequenced read" (runST (do
        arr <- newSTArray 3 7
        readSTArray arr 9 >> pure 1))
    attempt "final read" (runST (do
        arr <- newSTArray 3 7
        readSTArray arr 9))
    attempt "stored read action in range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        let rd = readSTArray arr 1
        a <- rd
        b <- rd
        pure (a + b)))
    attempt "stored read action out of range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        let rd = readSTArray arr 3
        a <- rd
        pure a))
    attempt "stored write action in range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        let wr = writeSTArray arr 2 5
        wr
        readSTArray arr 2))
    attempt "stored write action out of range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        let wr = writeSTArray arr 3 5
        wr
        readSTArray arr 2))
    attempt "stored modify action out of range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        let bump = modifySTArray arr (-1) (+ 1)
        bump
        readSTArray arr 2))
    attempt "stored constructor read at length" (runST (do
        let mk = newSTArray 3 7
        arr <- mk
        other <- mk
        writeSTArray other 0 1
        a <- readSTArray arr 0
        v <- readSTArray arr 3
        pure (a + v)))
    attempt "stored constructor write in range" (runST (do
        let mk = newSTArray 3 7
        arr <- mk
        other <- mk
        writeSTArray other 2 1
        a <- readSTArray arr 2
        b <- readSTArray other 2
        pure (a * 10 + b)))
    attempt "stored list constructor write at length" (runST (do
        let mk = newSTArrayFromList [1, 2]
        arr <- mk
        writeSTArray arr 2 3
        stArrayLength arr))
    attempt "stored negative size constructor read" (runST (do
        let mk = newSTArray (-2) 7
        arr <- mk
        v <- readSTArray arr 0
        pure v))
    attempt "passed read action in range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        twice (readSTArray arr 1)))
    attempt "passed read action at length" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        twice (readSTArray arr 3)))
    attempt "passed read action negative" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        twice (readSTArray arr (-1))))
    attempt "passed write action in range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        through (writeSTArray arr 2 5)
        readSTArray arr 2))
    attempt "passed write action at length" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        through (writeSTArray arr 3 5)
        readSTArray arr 2))
    attempt "passed modify action in range" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        through (modifySTArray arr 0 (+ 1))
        readSTArray arr 0))
    attempt "passed modify action at length" (runST (do
        arr <- newSTArrayFromList [10, 20, 30]
        through (modifySTArray arr 3 (+ 1))
        readSTArray arr 0))
    attempt "passed constructor read at length" (runST (do
        arr <- through (newSTArray 3 7)
        a <- readSTArray arr 2
        n <- through (stArrayLength arr)
        v <- readSTArray arr n
        pure (a + v)))
    attempt "passed constructor write at length" (runST (do
        arr <- through (newSTArray 3 7)
        writeSTArray arr 3 1
        readSTArray arr 0))
    attempt "passed list constructor write at length" (runST (do
        arr <- through (newSTArrayFromList [1, 2])
        writeSTArray arr 2 3
        stArrayLength arr))
    attemptList "passed list constructor elements" (runST (do
        arr <- through (newSTArrayFromList [1, 2])
        writeSTArray arr 1 3
        through (stArrayToList arr)))
    attempt "passed negative size constructor read" (runST (do
        arr <- through (newSTArray (-2) 7)
        v <- readSTArray arr 0
        pure v))
    attempt "forM_ writes in range" (runST (do
        arr <- newSTArray 3 0
        forM_ [0 .. 2] (\i -> writeSTArray arr i (i + 1))
        sumThrough arr 0 2 0))
    attempt "forM_ writes one past" (runST (do
        arr <- newSTArray 3 0
        forM_ [0 .. 3] (\i -> writeSTArray arr i (i + 1))
        sumThrough arr 0 2 0))
    attempt "forM_ modifies one past" (runST (do
        arr <- newSTArray 3 0
        forM_ [0 .. 3] (\i -> modifySTArray arr i (+ i))
        sumThrough arr 0 2 0))
    attempt "when write at length" (runST (do
        arr <- newSTArray 3 0
        n <- stArrayLength arr
        when (n == 3) (writeSTArray arr n 1)
        pure n))
    attempt "when write skipped" (runST (do
        arr <- newSTArray 3 0
        n <- stArrayLength arr
        when (n /= 3) (writeSTArray arr n 1)
        pure n))
    attempt "stored suspension read" (runST (do
        arr <- newSTArray 3 0
        writeSTArray arr 1 (sum [1 .. 10])
        a <- readSTArray arr 1
        b <- readSTArray arr 1
        pure (a + b)))
    attemptList "elements after in-range writes" (runST (do
        arr <- newSTArray 3 7
        writeSTArray arr 0 1
        modifySTArray arr 2 (+ 1)
        stArrayToList arr))
    r <- try (runST (do
        arr <- newSTArray 2 0
        v <- readSTArray arr 2
        pure v) `seq` pure ())
    case r of
        Left msg -> putStrLn ("try: Left " <> msg)
        Right _ -> putStrLn "try: Right"
    ok <- try (runST (do
        arr <- newSTArray 2 0
        v <- readSTArray arr 1
        pure v) `seq` pure ())
    case ok of
        Left msg -> putStrLn ("try in range: Left " <> msg)
        Right _ -> putStrLn "try in range: Right"
