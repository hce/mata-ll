-- With `import Data.List` in scope, `sort` and `sortBy` are the same
-- functions as the Prelude's — GHC 9.14.1's Data.OldList algorithm — so
-- the swapped-argument comparator (M7), NaN under `sort` and under
-- `sortBy compare` (N8), constant and never-EQ comparators and stability
-- print as without the import; Data.List used to carry a second `sortBy`
-- of a different shape that won through the flat namespace.
import Data.List

dv :: Number -> Number -> Number
dv a b = a / b

main :: IO ()
main = do
    print (sortBy (\a b -> if fst a <= fst b then LT else GT) [(1, "a"), (0, "z"), (1, "b"), (1, "c")])
    print (sortBy (\_ _ -> GT) [1, 2, 3 :: Int])
    print (sortBy (\_ _ -> LT) [1, 2, 3 :: Int])
    print (sortBy (\_ _ -> EQ) [3, 1, 2 :: Int])
    print (sortBy (\_ _ -> GT) [1 .. 12 :: Int])
    let nan = dv 0.0 0.0
    print (sort [2.0, nan, 1.0])
    print (sort [nan, 3.0, 1.0, 2.0])
    print (sort [3.0, 1.0, nan, 2.0, nan, 0.5])
    print (sort [3.0, 0.0, 0.0, 0.0, 1.0, nan, 0.0])
    print (sort [nan, 0.0, 2.0, 6.0, nan, 4.0, 1.0])
    print (sort [1.0, nan, 2.0, 4.0, nan, 6.0, 0.0, 5.0, nan, nan, 4.0])
    print (sortBy compare [3.0, 1.0, nan, 2.0, nan, 0.5])
    print (sortBy compare [1.0, nan, 2.0, 4.0, nan, 6.0, 0.0, 5.0, nan, nan, 4.0])
    print (sortBy (\a b -> if a < b then GT else LT) [0, 2, 2, 2, 7, 9, 1, 8, 0, 5, 8, 8 :: Int])
    print (sortBy (\a b -> if a == b then GT else compare a b) [7, 1, 8, 0, 3, 3, 4, 0, 1, 8, 7 :: Int])
    print (sortBy (\a b -> compare (fst a) (fst b)) [(2, "b"), (1, "x"), (2, "a"), (1, "y"), (2, "c")])
    print (sort [5, 3, 9, 1, 3, 7, 2, 8, 6, 4 :: Int])
    print (sortBy (\a b -> compare (a `mod` 3) (b `mod` 3)) [1 .. 10 :: Int])
    print (sort ["pear", "apple", "fig", "apple", "kiwi"])
    print (length (sort [100000, 99999 .. 1 :: Int]))
