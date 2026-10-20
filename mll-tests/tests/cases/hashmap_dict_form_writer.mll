-- W24 (H2, H3): a map written by a DICTIONARY-FORM body is read by
-- specialized ones. `widen` uses M.insert at 17 value types, past the
-- specialization limit, so every M.insert below runs the dictionary-form
-- body of Data.Map's insert — its key encoder comes from the key type's
-- Hashable dictionary, built at the call site — while M.lookup, M.member,
-- M.delete, M.keys and M.toList stay specialized. Every key shape the
-- encoder composes: scalars, lists (literal and lazily built), nested
-- lists, Maybe, nested Maybe, tuples, tuples of Maybe, lists of strings.
module Main where

import qualified Data.Map as M

widen :: Int
widen = M.size (M.insert 1 (1 :: Int) M.empty) + M.size (M.insert 1 (2.5 :: Number) M.empty) + M.size (M.insert 1 ("s" :: String) M.empty) + M.size (M.insert 1 (True :: Bool) M.empty) + M.size (M.insert 1 ([1] :: [Int]) M.empty) + M.size (M.insert 1 (Just 1 :: Maybe Int) M.empty) + M.size (M.insert 1 ((1, 2) :: (Int, Int)) M.empty) + M.size (M.insert 1 (["a"] :: [String]) M.empty) + M.size (M.insert 1 (Just "b" :: Maybe String) M.empty) + M.size (M.insert 1 ((1, "c") :: (Int, String)) M.empty) + M.size (M.insert 1 ([True] :: [Bool]) M.empty) + M.size (M.insert 1 (Just False :: Maybe Bool) M.empty) + M.size (M.insert 1 ((True, 3) :: (Bool, Int)) M.empty) + M.size (M.insert 1 ([[4]] :: [[Int]]) M.empty) + M.size (M.insert 1 (Just (Just 5) :: Maybe (Maybe Int)) M.empty) + M.size (M.insert 1 ((6, 7, 8) :: (Int, Int, Int)) M.empty) + M.size (M.insert 1 ([9.5] :: [Number]) M.empty)

main :: IO ()
main = do
    print widen
    let mi = M.insert (3 :: Int) "c" (M.insert 1 "a" (M.insert 2 "b" M.empty))
    print (M.lookup 1 mi, M.member 2 mi, M.member 9 mi, M.size mi)
    print (M.toList mi)
    let ms = M.insert "b" 2 (M.insert "a" (1 :: Int) M.empty)
    print (M.lookup "a" ms, M.lookup "zz" ms, M.keys ms)
    let mb = M.insert True "t" (M.insert False "f" M.empty)
    print (M.lookup False mb, M.toList mb)
    let mn = M.insert (2.5 :: Number) "x" (M.insert 1.5 "y" M.empty)
    print (M.lookup 1.5 mn, M.lookup 2.0 mn, M.keys mn)
    let ml = M.insert [2 :: Int] "two" (M.insert [1, 2] "onetwo" (M.insert [1] "one" (M.insert [] "nil" M.empty)))
    print (M.lookup [1, 2] ml, M.lookup [] ml, M.lookup [2, 1] ml, M.member [1] ml)
    print (M.keys ml)
    print (M.toList (M.delete [1] ml))
    let mz = M.insert (map (+ 1) [1, 2 :: Int]) "mapped" (M.insert [1 .. 3] "range" (M.insert ([4] ++ [5]) "app" M.empty))
    print (M.lookup [2, 3] mz, M.lookup [1, 2, 3] mz, M.lookup [4, 5] mz, M.size mz)
    print (M.lookup (take 2 [7 ..]) (M.insert [7, 8 :: Int] "taken" M.empty))
    let mll = M.insert [[1 :: Int], [2, 3]] "nested" (M.insert [[]] "emptyinner" M.empty)
    print (M.lookup [[1], [2, 3]] mll, M.lookup [[]] mll, M.lookup [] mll, M.keys mll)
    let mm = M.insert (Just (7 :: Int)) "just" (M.insert Nothing "nothing" M.empty)
    print (M.lookup (Just 7) mm, M.lookup Nothing mm, M.lookup (Just 8) mm, M.keys mm)
    let mmm = M.insert (Just (Just (1 :: Int))) "jj" (M.insert (Just Nothing) "jn" (M.insert Nothing "n" M.empty))
    print (M.lookup (Just (Just 1)) mmm, M.lookup (Just Nothing) mmm, M.lookup Nothing mmm, M.keys mmm)
    let mt = M.insert (1 :: Int, Just (2 :: Int)) "a" (M.insert (1, Nothing) "b" (M.insert (0, Just 9) "c" M.empty))
    print (M.lookup (1, Just 2) mt, M.lookup (1, Nothing) mt, M.lookup (2, Nothing) mt, M.keys mt)
    let m3 = M.insert (1 :: Int, 2 :: Int, 3 :: Int) "123" (M.insert (1, 2, 4) "124" M.empty)
    print (M.lookup (1, 2, 3) m3, M.lookup (1, 2, 4) m3, M.lookup (3, 2, 1) m3, M.toList m3)
    let mss = M.insert ["a,b", "c"] 1 (M.insert ["a", "b,c"] (2 :: Int) M.empty)
    print (M.lookup ["a", "b,c"] mss, M.lookup ["a,b", "c"] mss, M.size mss, M.keys mss)
    let mf = M.insert (Just (2 :: Int)) "two" (M.fromList [(Just 1, "one"), (Nothing, "none")])
    print (M.toList mf, M.size (M.insert (Just 1) "uno" mf))
    print (M.size (M.insert [1 :: Int] "again" ml), M.size (M.delete [] ml), M.member [] (M.delete [] ml))
