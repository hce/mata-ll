-- List-fusion growth (fuse.rs round 2): sum/length consumers, the take
-- stage, operator and lambda fold functions — and the stage-strictness
-- gates that keep fusion faithful to lazy demand. Every printed value is
-- GHC-goldened; the error-carrying shapes pin that a fused (or declined)
-- pipeline demands EXACTLY what the lazy one demanded.

import Data.List (foldl')

step :: Int -> Int -> Int
step a x = (a + x) `mod` 1000000007

addk :: Int -> Int -> Int
addk k x = k + x

main :: IO ()
main = do
    -- Consumers.
    print (sum (map (* 2) (filter even [1 .. 100 :: Int])))
    print (sum (filter (> 2) [3, 1, 4, 1, 5 :: Int]))
    print (length (filter odd (map (+ 1) [1 .. 50 :: Int])))
    -- length demands no element values: the map's output is undemanded
    -- (dropped when fused), and the range is a pure count.
    print (length (map (* 9) [1 .. 7 :: Int]))
    -- The fold function as an operator, a lambda, and a partial
    -- application (the row's tail covers the remaining parameter).
    print (foldl' (+) 0 (map (* 3) [1 .. 20 :: Int]))
    print (foldl' (\a x -> a + x * 2) 0 (filter odd [1 .. 20 :: Int]))
    print (foldl' step 0 (map (addk 7) [1 .. 10 :: Int]))
    -- take: the budget stops the pipeline exactly where laziness did.
    print (foldl' (+) 0 (take 2 [1, 2, error "beyond"]))
    print (foldl' (+) 0 (take 0 [error "head" :: Int]))
    print (foldl' (+) 0 (take (-1) [error "neg" :: Int]))
    print (foldl' (+) 0 (take 99 [1, 2, 3 :: Int]))
    print (foldl' step 0 (take 3 (map (* 2) (filter odd [1 .. 100 :: Int]))))
    print (sum (take 5 (filter even [1 .. 100 :: Int])))
    print (length (take 4 [1 .. 100 :: Int]))
    -- The element that spends the last budget must not pull the next
    -- cell: the spine beyond the second survivor is an error.
    print (foldl' (+) 0 (take 2 (filter odd (1 : 3 : error "past-survivors"))))
    -- take inside the stages: budget counts pre-filter elements.
    print (sum (filter odd (take 6 [1 .. 100 :: Int])))
    -- Stage gates: a LAZY stage function must decline fusion — these
    -- results only exist because the element bottoms stay undemanded.
    print (foldl' step 0 (map (\_ -> 1) [error "b1", error "b2"]))
    print (foldl' step 0 (filter (\_ -> False) [error "b3" :: Int]))
    -- A composed stage function (the demand analyzer's `(.)` rule) and a
    -- point-free named one (`odd = not . even`, an eta-padded row).
    print (sum (filter (not . even) [1 .. 30 :: Int]))
    print (length (filter odd [1 .. 30 :: Int]))
    -- Values the loop consumes natively arrive as do-bound THUNKS: the
    -- take budget, the range bounds, and a native fold's initial
    -- accumulator (the fuzzer's find at batch index 1941).
    n <- return (2 + 1 :: Int)
    lo <- return (1 :: Int)
    hi <- return (10 :: Int)
    z0 <- return (5 :: Int)
    print (foldl' (+) z0 (take n [lo .. hi]))
    print (sum (take n (map (* 2) [lo .. hi])))
    print (length (filter odd [lo .. hi]))
    -- A lambda map stage returning a CAPTURED lazy value: strict in its
    -- parameter (fuses), but its result is a raw thunk the native sum
    -- step must force.
    let lazyv = sum [1 .. 10 :: Int]
    print (sum (map (\x -> if x > 2 then lazyv else x) [1 .. 5 :: Int]))
