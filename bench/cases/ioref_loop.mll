-- IO loop over a mutable cell: one modifyIORef' per iteration inside a
-- recursive IO action. The twin is a plain loop over a local — the
-- speed-of-light for "mutable state threaded through IO" — so the ratio
-- prices the IO plumbing plus the fused IORef intrinsics.
module Main where

import Data.IORef

go :: IORef Int -> Int -> IO ()
go _ 0 = pure ()
go r i = do
    modifyIORef' r (\v -> (v + i) `mod` 1000000007)
    go r (i - 1)

main :: IO ()
main = do
    r <- newIORef 0
    go r 1000000
    v <- readIORef r
    print v
