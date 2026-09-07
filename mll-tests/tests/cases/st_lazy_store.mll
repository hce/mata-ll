-- GHC's boxed STArray stores values lazily: writing a bottom that is never
-- read is silent, a bottom initializer may be overwritten before any read,
-- and newSTArrayFromList does not evaluate the elements. The mata-ll
-- runtime used to force every stored value on write (the every-slot-WHNF
-- invariant), so `writeSTArray arr 1 (error "never read")` raised where
-- GHC is silent. A slot now holds the value as stored; readSTArray forces
-- the slot it returns.

main :: IO ()
main = do
  -- a bottom stored and never read is silent
  print (runST (do
    arr <- newSTArray 3 (0 :: Int)
    writeSTArray arr 1 (error "never read")
    a <- readSTArray arr 0
    c <- readSTArray arr 2
    pure (a + c)))
  -- a bottom initializer overwritten before any read
  print (runST (do
    arr <- newSTArray 2 undefined
    writeSTArray arr 0 1
    writeSTArray arr 1 2
    x <- readSTArray arr 0
    y <- readSTArray arr 1
    pure (x + y)))
  -- lazy elements from a list: only the read ones are demanded
  print (runST (do
    arr <- newSTArrayFromList [10, error "unread element", 30]
    x <- readSTArray arr 0
    z <- readSTArray arr 2
    pure (x + z)))
  -- a stored suspension is evaluated when read; modify sees the value
  print (runST (do
    arr <- newSTArray 1 (0 :: Int)
    writeSTArray arr 0 (sum [1 .. 100])
    modifySTArray arr 0 (* 2)
    v <- readSTArray arr 0
    pure v))
  -- stArrayToList keeps an unread bottom unevaluated
  print (runST (do
    arr <- newSTArrayFromList [1, 2, 3]
    writeSTArray arr 1 (error "still unread")
    xs <- stArrayToList arr
    pure (head xs + last xs)))
  -- the shared initializer is evaluated once and seen by every slot
  print (runST (do
    arr <- newSTArray 3 (product [1 .. 10])
    a <- readSTArray arr 0
    b <- readSTArray arr 2
    pure (a - b)))
