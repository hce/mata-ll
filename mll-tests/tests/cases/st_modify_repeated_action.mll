-- Test: a FIRST-CLASS `modifySTArray` action run more than once modifies the
-- same index every time. The runtime closure rebound its own `idx` upvalue
-- (`idx = __force(idx) + 1`) on each run, so the second run of one stored
-- action modified index+1, the third index+2 — every other ST primitive
-- only re-forces its captured values, which is idempotent.

bump :: STArray s -> ST s ()
bump arr = modifySTArray arr 0 (\v -> v + 1)

runAll :: [ST s ()] -> ST s ()
runAll [] = pure ()
runAll (a:as) = do
    a
    runAll as

main :: IO ()
main = do
    let r = runST (do
            arr <- newSTArray 3 0
            let act = modifySTArray arr 0 (\v -> v + 10)
            act
            act
            act
            a <- readSTArray arr 0
            b <- readSTArray arr 1
            c <- readSTArray arr 2
            pure (a, b, c))
    assert (r == (30, 0, 0)) "one stored modify action, run three times, hits index 0 each time"
    let s = runST (do
            arr <- newSTArray 2 5
            let act = bump arr
            act
            act
            x <- readSTArray arr 0
            y <- readSTArray arr 1
            pure (x, y))
    assert (s == (7, 5)) "a function-returned modify action, run twice, hits index 0 each time"
    -- The primitive taken as a first-class VALUE (partially applied) is the
    -- runtime's closure form; the stored action is one closure, run thrice.
    let t = runST (do
            arr <- newSTArray 3 1
            let m = modifySTArray arr
            let act = m 0 (\v -> v * 2)
            act
            act
            act
            a <- readSTArray arr 0
            b <- readSTArray arr 1
            c <- readSTArray arr 2
            pure (a, b, c))
    assert (t == (8, 1, 1)) "the first-class primitive's closure, run three times, hits index 0 each time"
    -- The closure stored in a data structure (a list element is evaluated
    -- once and the SAME closure object is run on every traversal).
    let u = runST (do
            arr <- newSTArray 3 1
            let m = modifySTArray arr
            let acts = [m 0 (\v -> v + 100), m 1 (\v -> v + 1)]
            runAll acts
            runAll acts
            a <- readSTArray arr 0
            b <- readSTArray arr 1
            c <- readSTArray arr 2
            pure (a, b, c))
    assert (u == (201, 3, 1)) "stored closures traversed twice hit their own indices both times"
