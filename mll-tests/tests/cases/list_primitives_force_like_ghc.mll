-- The runtime list primitives force exactly what GHC's definitions
-- force, in GHC's order. `take n (x : xs) = x : take (n - 1) xs` forces
-- the one cell it returns and suspends the rest, so a prefix over a list
-- whose tail is a suspension -- a parameter, a bottom, a knot that
-- defines the tail through the take itself -- demands nothing past the
-- cells consumed; the realized-spine walk stops at the first suspended
-- tail. `zipWith f (x : xs) (y : ys) = f x y : zipWith f xs ys` matches
-- its first list first (`zipWith f [] ⊥ = []`) and suspends both tails,
-- so a first list that ends before the second is zipped without the
-- second's tail ever being forced. splitAt is GHC's own definition:
-- strict in n and, for n > 0, in the list to WHNF of the pair, with the
-- pair's parts lazy. `xs !! n` forces the index and its sign check
-- before the list. zip, zip3, zipWith3 (the Prelude's own clauses),
-- drop, takeWhile, replicate, last and init are pinned at the same
-- shapes, each consumer fused (sum/length over a take, a map or a leaf)
-- and unfused (show/head). Regression: take's realized-spine arm forced
-- up to n suspended tails while building the prefix (`head (take 2 (1 :
-- ⊥))` raised; the knot `xs = 1 : ys; ys = take 3 xs` overflowed the
-- stack); zipWith forced its second list before testing the first for
-- [] and both tails before testing the first's (`length (zipWith (+) []
-- ⊥)` and `zipWith (+) [1] (2 : ⊥)` raised); splitAt was the Report's
-- lazy pair `(take n xs, drop n xs)`; the list operand of `!!` was
-- evaluated as a Lua argument before the index.

module Main where

import Data.List (foldl')

f :: [Int] -> Int
f rest = head (take 2 (1 : rest))

bottomList :: [Int]
bottomList = error "src"

tailBottom :: [Int]
tailBottom = 1 : error "tl"

tailBottom2 :: [Int]
tailBottom2 = 1 : 2 : error "tl2"

fibs :: [Int]
fibs = 0 : 1 : zipWith (+) fibs (tail fibs)

knotTake :: [Int]
knotTake = xs
  where
    xs = 1 : ys
    ys = take 3 xs

knotZip :: [Int]
knotZip = ys
  where
    ys = 1 : zipWith (+) ys (take 2 ys)

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
    probe (f (error "boom"))
    probe (head (take 2 tailBottom))
    probe (head (take 3 tailBottom))
    probe (length (take 3 tailBottom))
    probeS (show (take 2 tailBottom2))
    probeS (show (take 1 tailBottom))
    probe (sum (take 2 tailBottom2))
    probe (length (take 2 tailBottom2))
    probe (sum (take 3 tailBottom))
    probe (foldl' (+) 0 (take 1 tailBottom))
    probe (head (take (error "tn") tailBottom))
    probe (length (take 0 bottomList))
    probeS (show knotTake)
    probe (sum knotTake)
    let xs = 1 : ys
        ys = take 3 xs
    probeS (show xs)
    probe (length (zipWith (+) [] (error "boom2" :: [Int])))
    probe (length (zipWith (+) (error "l" :: [Int]) []))
    probeS (show (zipWith (+) [1] (2 : error "b")))
    probe (length (zipWith (+) [1] (2 : error "b")))
    probe (sum (zipWith (+) [1] (2 : error "b")))
    probe (sum (take 1 (zipWith (+) [1, 2] (2 : error "b"))))
    probe (sum (map (+ 1) (zipWith (+) [1] (2 : error "b"))))
    probe (length (zipWith (+) (1 : error "a") (2 : error "b")))
    probe (head (zipWith (+) (1 : error "a") (2 : error "b")))
    probeS (show (zipWith (+) [1, 2] (3 : error "b2")))
    probeS (show (take 10 fibs))
    probe (sum (take 10 fibs))
    probeS (show knotZip)
    probe (fst (head (zip [1, 2] (3 : error "z"))))
    probe (length (zip [1] (3 : error "z")))
    probe (length (zip [1, 2] (3 : error "z")))
    probe (length (zip ([] :: [Int]) (error "z2" :: [Int])))
    probe (length (zip (error "z1" :: [Int]) ([] :: [Int])))
    probe (length (zip3 [1] [2] (3 : error "c")))
    probe (length (zip3 ([] :: [Int]) (error "a" :: [Int]) (error "b" :: [Int])))
    probe (length (zip3 [1] ([] :: [Int]) (error "b" :: [Int])))
    probe (length (zip3 [1] (error "b" :: [Int]) ([] :: [Int])))
    probeS (show (zip3 [1, 2] [3] (4 : error "c")))
    probeS (show (zipWith3 (\a b c -> a + b + c) [1] (2 : error "x") [3]))
    probe (length (zipWith3 (\a b c -> a + b + c) ([] :: [Int]) (error "x" :: [Int]) (error "y" :: [Int])))
    probe (head (drop 1 tailBottom2))
    probe (length (drop 0 bottomList))
    probeS (show (drop 1 tailBottom2))
    probe (case splitAt (error "n") [1 :: Int] of (_, _) -> 0)
    probe (case splitAt 1 bottomList of (_, _) -> 0)
    probe (case splitAt 0 bottomList of (_, _) -> 0)
    probeS (show (fst (splitAt 1 tailBottom)))
    probe (head (snd (splitAt 1 tailBottom2)))
    probe (head (fst (splitAt 3 tailBottom)))
    probe (length (snd (splitAt 0 tailBottom)))
    probe (tailBottom !! 0)
    probe ([1, 2] !! 1)
    probe ((error "ix" :: [Int]) !! (error "n"))
    probe (tailBottom2 !! 1)
    probeS (show (takeWhile (< 3) (1 : 2 : 3 : error "tw")))
    probe (head (takeWhile (< 3) tailBottom))
    probe (length (takeWhile (< 3) tailBottom))
    probe (length (replicate 3 (error "r" :: Int)))
    probe (length (replicate (error "rn") (1 :: Int)))
    probe (last tailBottom)
    probe (last [1, 2])
    probeS (show (init [1 :: Int]))
    probe (head (init tailBottom2))
    probe (length (init tailBottom2))

-- expect: 1
-- expect: 1
-- expect: 1
-- expect: tl
-- expect: [1,2]
-- expect: [1]
-- expect: 3
-- expect: 2
-- expect: tl
-- expect: 1
-- expect: tn
-- expect: 0
-- expect: [1,1,1,1]
-- expect: 4
-- expect: [1,1,1,1]
-- expect: 0
-- expect: l
-- expect: [3]
-- expect: 1
-- expect: 3
-- expect: 3
-- expect: 4
-- expect: a
-- expect: 3
-- expect: b2
-- expect: [0,1,1,2,3,5,8,13,21,34]
-- expect: 88
-- expect: [1,2,4]
-- expect: 1
-- expect: 1
-- expect: z
-- expect: 0
-- expect: z1
-- expect: 1
-- expect: 0
-- expect: 0
-- expect: b
-- expect: [(1,3,4)]
-- expect: [6]
-- expect: 0
-- expect: 2
-- expect: src
-- expect: tl2
-- expect: n
-- expect: src
-- expect: 0
-- expect: [1]
-- expect: 2
-- expect: 1
-- expect: tl
-- expect: 1
-- expect: 2
-- expect: n
-- expect: 2
-- expect: [1,2]
-- expect: 1
-- expect: tl
-- expect: 3
-- expect: rn
-- expect: tl
-- expect: 2
-- expect: []
-- expect: 1
-- expect: tl2
