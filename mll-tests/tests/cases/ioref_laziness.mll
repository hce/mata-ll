-- GHC laziness parity for Data.IORef, pinned against the oracle:
-- newIORef and writeIORef don't force the value (a stored bottom that is
-- overwritten before any read must not raise), modifyIORef doesn't call f
-- at modify time (it stores the unevaluated `f old`), and modifyIORef'
-- forces the new value to WHNF ONLY — a pair whose first component is
-- bottom stores fine, and reading its second component succeeds. The
-- raising half of modifyIORef' (a bomb that IS forced) is pinned
-- mechanically by the strictness-contract harness, not here.

module Main where
import Data.IORef

runAll :: [IO ()] -> IO ()
runAll [] = pure ()
runAll (a:as) = a >> runAll as

main :: IO ()
main = do
    r <- newIORef (0 :: Int)
    writeIORef r (error "boom")
    writeIORef r 7
    x <- readIORef r
    print x
    modifyIORef r (\_ -> error "boom2")
    writeIORef r 9
    y <- readIORef r
    print y
    b <- newIORef (error "boom3" :: Int)
    writeIORef b 11
    z <- readIORef b
    print z
    -- first-class write actions built, stored, run in order later
    let acts = [writeIORef r 1, writeIORef r 2]
    runAll acts
    w <- readIORef r
    print w
    -- modifyIORef' stops at WHNF: the pair constructor is the forced
    -- layer, its bottom field is never touched
    pr <- newIORef ((0, 0) :: (Int, Int))
    modifyIORef' pr (\_ -> (error "l", 2))
    p <- readIORef pr
    print (snd p)

-- expect: 7
-- expect: 9
-- expect: 11
-- expect: 2
-- expect: 2
