-- The lazy generics are lazy in their FUNCTION argument and (map,
-- zipWith) in each head, exactly as in GHC: `f x` is suspended per
-- element, f itself is forced only by the first demanded application,
-- and filter forces its predicate only when an element exists to test.
-- Every error in this file must stay untouched — the printed values
-- only exist because the demand never reaches them.

import Data.List (foldl')

step :: Int -> Int -> Int
step a x = a + x

main :: IO ()
main = do
    -- A spine consumer never applies (or forces) a map's function.
    print (length (map (error "map-fn") ([1, 2, 3] :: [Int])))
    -- A lazy fold function leaves the map's per-head suspensions unrun.
    print (foldl' step 0 (map (\_ -> 1) [error "elem1", error "elem2"]))
    -- A rejected element's head suspension is never demanded either.
    print (foldl' step 0 (filter (\_ -> False) (map (\_ -> error "mapped") [1 .. 5 :: Int])))
    -- filter/foldr/foldl on an empty structure never force the function.
    print (foldl' step 0 (filter (error "filter-fn") ([] :: [Int])))
    print (foldr (error "foldr-fn") (7 :: Int) ([] :: [Int]))
    print (foldl (error "foldl-fn") (8 :: Int) ([] :: [Int]))
    -- zipWith is lazy in f the same way; the shorter operand ends the
    -- walk before any application.
    print (length (zipWith (error "zip-fn") [1, 2, 3 :: Int] [4, 5 :: Int]))
    -- Suspended heads are call-by-need: one application per demanded
    -- head, results shared (the double read must not double-apply,
    -- observable through the fold summing the same cell twice).
    let ys = map (* 10) [1, 2, 3 :: Int]
    print (foldl' step 0 ys + foldl' step 0 ys)
    -- take never demands heads: the map's function stays unapplied even
    -- though the spine advances through it.
    print (length (take 2 (map (error "taken-fn") [1, 2, 3 :: Int])))
