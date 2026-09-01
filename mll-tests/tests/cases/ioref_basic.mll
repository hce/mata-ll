-- Data.IORef basics: read/write/modify/modify', the counter-loop shape the
-- feature exists for, Eq as pointer identity (GHC's `instance Eq (IORef a)`
-- — two cells with equal contents are not ==), refs as first-class values
-- (lists, Maybe, ref-in-ref, captured in a closure run twice), and
-- nil-represented values (Nothing, [], ()) surviving the slot — a ref
-- stores into a fixed slot, so Lua's t[k]=nil delete semantics never apply.

module Main where
import Data.IORef

sumLoop :: IORef Int -> Int -> IO Int
sumLoop r 0 = readIORef r
sumLoop r n = do
    modifyIORef' r (+ n)
    sumLoop r (n - 1)

main :: IO ()
main = do
    r <- newIORef (10 :: Int)
    x <- readIORef r
    print x
    writeIORef r 20
    y <- readIORef r
    print y
    modifyIORef r (* 2)
    z <- readIORef r
    print z
    total <- sumLoop r 100
    print total
    -- Eq is pointer identity, not content equality
    s <- newIORef (5090 :: Int)
    print (r == r)
    print (r == s)
    print (r /= s)
    -- refs are ordinary values: containers and Eq-constrained prelude fns
    let refs = [r, s]
    print (length refs)
    print (elem r refs)
    -- ref-in-ref
    rr <- newIORef r
    inner <- readIORef rr
    v <- readIORef inner
    print v
    -- a ref reached through Maybe; writing through the alias is visible
    -- through the original
    let mb = Just s
    case mb of
        Just t -> do
            writeIORef t 77
            w <- readIORef s
            print w
        Nothing -> pure ()
    -- a stored first-class action performed twice
    let bump = modifyIORef' r (+ 1)
    bump
    bump
    fin <- readIORef r
    print fin
    -- String ref through the lazy modify
    sr <- newIORef "a"
    modifyIORef sr (<> "b")
    str <- readIORef sr
    putStrLn str
    -- nil-represented values keep their slot
    nr <- newIORef (Nothing :: Maybe Int)
    n0 <- readIORef nr
    print n0
    writeIORef nr (Just 3)
    n1 <- readIORef nr
    print n1
    writeIORef nr Nothing
    n2 <- readIORef nr
    print n2
    ur <- newIORef ()
    u <- readIORef ur
    print u
    lr <- newIORef ([] :: [Int])
    l <- readIORef lr
    print l

-- expect: 10
-- expect: 20
-- expect: 40
-- expect: 5090
-- expect: True
-- expect: False
-- expect: True
-- expect: 2
-- expect: True
-- expect: 5090
-- expect: 77
-- expect: 5092
-- expect: ab
-- expect: Nothing
-- expect: Just 3
-- expect: Nothing
-- expect: ()
-- expect: []
