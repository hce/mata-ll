-- W24 (H2, H3): a Hashable-constrained body compiled with dictionary
-- passing. `probe` is used at 19 key types, so its own body — and
-- `build`, `paired` and the Data.Map operations it calls at its key
-- variable — run in dictionary form: every map operation reads the key
-- type's encoder and order off the Hashable dictionary in scope, the
-- parameter for `k` itself and a dictionary composed over it for the
-- shapes `(k, Maybe k)` and `[k]`. `Box` captures the dictionary of its
-- hidden key type at construction; a match uses the captured one.
module Main where

import qualified Data.Map as M

data Box = forall k. Hashable k => Box k

build :: Hashable k => [k] -> Int -> M.Map k Int -> M.Map k Int
build [] _ m = m
build (k : ks) i m = build ks (i + 1) (M.insert k i m)

paired :: Hashable k => k -> k -> (Maybe String, Maybe String, Maybe String, [(k, Maybe k)], [[k]])
paired a b =
    let mp = M.insert (a, Just b) "ab" (M.insert (b, Nothing) "bn" M.empty)
        ml = M.fromList [([a, b], "ab"), ([b], "b"), ([], "nil")]
    in (M.lookup (a, Just b) mp, M.lookup (b, Nothing) mp, M.lookup [a, b] ml, M.keys mp, M.keys ml)

probe :: Hashable k => [k] -> k -> ((Maybe Int, Bool, Int, [k]), (Maybe String, Maybe String, Maybe String, [(k, Maybe k)], [[k]]))
probe ks k =
    let m = build ks 1 M.empty
    in ((M.lookup k m, M.member k m, M.size m, M.keys m), paired k (head ks))

table :: Hashable k => [k] -> [(k, Int)]
table ks = M.toList (M.delete (head ks) (build ks 1 M.empty))

boxedSize :: Box -> Int
boxedSize (Box k) = M.size (M.insert k 1 (M.insert k 2 M.empty))

boxedCount :: Box -> Int
boxedCount (Box k) = M.size (M.fromList [(k, 1 :: Int), (k, 2)])

boxedHit :: Box -> Bool
boxedHit (Box k) = M.member k (M.singleton k True)

report :: (Show a, Show b) => (a, b) -> IO ()
report (p, q) = do
    print p
    print q

main :: IO ()
main = do
    report (probe [3, 1, 2 :: Int] 2)
    report (probe [2.5, 1.5 :: Number] 1.5)
    report (probe ["b", "a"] "c")
    report (probe [True, False] False)
    report (probe [[2], [1, 2], [1 :: Int]] [1, 2])
    report (probe [Just (2 :: Int), Nothing, Just 1] Nothing)
    report (probe [(1 :: Int, 2 :: Int), (0, 5)] (0, 5))
    report (probe [["x", "y"], ["x"]] ["x"])
    report (probe [Just "b", Nothing] (Just "b"))
    report (probe [(2 :: Int, "b"), (1, "z")] (1, "z"))
    report (probe [[True], [False, True]] [True])
    report (probe [Just False, Just True] Nothing)
    report (probe [(True, 1 :: Int), (False, 2)] (True, 1))
    report (probe [[[1 :: Int]], [[]], []] [[]])
    report (probe [Just (Just (1 :: Int)), Just Nothing] (Just Nothing))
    report (probe [(1 :: Int, 2 :: Int, 3 :: Int), (1, 2, 0)] (1, 2, 3))
    report (probe [[1.5 :: Number], [0.5]] [0.5])
    report (probe [Just [1 :: Int], Nothing] (Just [1]))
    report (probe [(1 :: Int, [2 :: Int]), (1, [])] (1, []))
    print (table [2, 1 :: Int])
    print (table [[1 :: Int], []])
    print (table [Just "a", Nothing])
    print (table [map (+ 1) [1, 2 :: Int], [2, 3], [3] ++ [4]])
    print (boxedSize (Box (1 :: Int)), boxedSize (Box [1, 2 :: Int]), boxedSize (Box (Just "s")), boxedSize (Box (1 :: Int, 2 :: Int)))
    print (boxedCount (Box [[1 :: Int]]), boxedCount (Box (map (* 2) [1, 2 :: Int])), boxedHit (Box (Just (Just (3 :: Int)))), boxedHit (Box (True, "t")))
