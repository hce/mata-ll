-- List-pipeline fusion (fuse.rs): a `take` stage's exhaustion check is
-- evaluated at the budget's own position, BEFORE anything inside the
-- take is evaluated -- the inner stages, inner budgets, and the source
-- list or range bounds -- exactly as the lazy `take n xs` decides on n
-- before it touches xs. A spent budget (0, from `length []`) over a
-- bottom source therefore returns the seed; a bottom budget still
-- raises first; a nested inner budget is never evaluated once an outer
-- one is spent. Regression: the fused loop bound the source (a leaf
-- list, or a range's bounds) above the loop whose head held the
-- checks, so `sum (take 0 (map g ⊥))` raised where GHC prints 0. The
-- unfused shapes (`show` is not a fusion consumer) pin that the lazy
-- pipeline agrees. The last lines pin the lambda strictness gate: a
-- lambda whose body is a `case` with an irrefutable first pattern is
-- NOT strict in its parameter (the gate claimed the scrutinee forced
-- and the fused loop forced every element), one whose first pattern
-- scrutinizes the value is, and a `let` binder shadowing a module
-- function is an unknown local value, not that function's demand row.

module Main where

import Data.List (foldl')

g :: Int -> Int
g x = x + 1

bottomList :: [Int]
bottomList = error "src"

lo :: Int -> Int
lo _ = error "lo"

tailBottom :: [Int]
tailBottom = 1 : error "tl"

zero :: Int
zero = length ([] :: [Int])

one :: Int
one = length [7 :: Int]

probe :: Int -> IO ()
probe v = do
    r <- try (v `seq` pure v)
    putStrLn (either id show r)

probeS :: String -> IO ()
probeS s = do
    r <- try ((s == s) `seq` pure s)
    putStrLn (either id id r)

main :: IO ()
main = do
    probe (sum (take zero (map g bottomList)))
    probe (length (take zero bottomList))
    probe (sum (take zero [lo 1 .. 5]))
    probe (sum (take one [lo 1 .. 5]))
    probe (length (take zero (filter even bottomList)))
    probe (foldl' (+) 0 (take zero bottomList))
    probe (sum (take zero [1 .. lo 1]))
    probe (length (take zero [lo 1 .. 5]))
    probe (foldl' (+) 0 (take zero [lo 1 .. 5]))
    probe (sum (take (error "k") [1, 2, 3]))
    probe (sum (take (error "k") bottomList))
    probe (sum (take 2 (map g (take zero bottomList))))
    probe (sum (take zero (map g (take 2 bottomList))))
    probe (sum (take zero (map g (take (error "k2") bottomList))))
    probe (sum (take 2 (map g (take (error "k2") bottomList))))
    probe (sum (take 2 (map g (take 1 tailBottom))))
    probe (sum (filter even (take zero bottomList)))
    probe (sum (take zero (filter even bottomList)))
    probe (sum (take zero tailBottom))
    probe (sum (take one tailBottom))
    probe (length (take one tailBottom))
    probe (sum (take one (map g tailBottom)))
    probe (sum (take 1 (filter odd tailBottom)))
    probe (sum (take 2 (map g [1, 2, 3])))
    probeS (show (take zero (map g bottomList)))
    probeS (show (take zero [lo 1 .. 5]))
    probeS (show (take one [lo 1 .. 5]))
    probeS (show (take one tailBottom))
    probeS (show (take (error "k") [1, 2, 3]))
    probeS (show (take zero (map g (take (error "k2") bottomList))))
    probeS (show (take 2 (map g (take (error "k2") bottomList))))
    probeS (show (take 2 (map g [1, 2, 3])))
    probe (sum (map (\x -> case x of y -> (1 :: Int)) [error "b", 2]))
    probe (sum (map (\x -> case x of _ -> (1 :: Int)) [error "b", 2]))
    probe (sum (map (\x -> case x of
                              0 -> 7
                              _ -> (1 :: Int)) [error "b", 2]))
    probe (length (filter (\x -> let g = \_ -> True in g x) [error "b", 2]))
    probeS (show (map (\x -> case x of y -> (1 :: Int)) [error "b", 2]))
    probeS (show (map (\x -> case x of
                                0 -> 7
                                _ -> (1 :: Int)) [error "b", 2]))

-- expect: 0
-- expect: 0
-- expect: 0
-- expect: lo
-- expect: 0
-- expect: 0
-- expect: 0
-- expect: 0
-- expect: 0
-- expect: k
-- expect: k
-- expect: 0
-- expect: 0
-- expect: 0
-- expect: k2
-- expect: 2
-- expect: 0
-- expect: 0
-- expect: 0
-- expect: 1
-- expect: 1
-- expect: 2
-- expect: 1
-- expect: 5
-- expect: []
-- expect: []
-- expect: lo
-- expect: [1]
-- expect: k
-- expect: []
-- expect: k2
-- expect: [2,3]
-- expect: 2
-- expect: 2
-- expect: b
-- expect: 2
-- expect: [1,1]
-- expect: b
