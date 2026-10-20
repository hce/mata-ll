-- W24 (H2, H3): maps written by SPECIALIZED bodies are read by
-- dictionary-form ones. The widening sums use M.lookup, M.member and
-- M.delete at 17 key types each, past the specialization limit, so every
-- use of them below runs the dictionary-form body (its key encoder read
-- off the Hashable dictionary the call site builds for the concrete key
-- type), while M.insert, M.fromList and M.singleton stay specialized.
module Main where

import qualified Data.Map as M

has :: Maybe Int -> Int
has Nothing = 0
has (Just _) = 1

bit :: Bool -> Int
bit False = 0
bit True = 1

widenLookup :: Int
widenLookup = has (M.lookup (1 :: Int) (M.empty :: M.Map Int Int)) + has (M.lookup (1.5 :: Number) (M.empty :: M.Map Number Int)) + has (M.lookup "s" (M.empty :: M.Map String Int)) + has (M.lookup True (M.empty :: M.Map Bool Int)) + has (M.lookup [1 :: Int] (M.empty :: M.Map [Int] Int)) + has (M.lookup (Just (1 :: Int)) (M.empty :: M.Map (Maybe Int) Int)) + has (M.lookup (1 :: Int, 2 :: Int) (M.empty :: M.Map (Int, Int) Int)) + has (M.lookup ["a"] (M.empty :: M.Map [String] Int)) + has (M.lookup (Just "b") (M.empty :: M.Map (Maybe String) Int)) + has (M.lookup (1 :: Int, "c") (M.empty :: M.Map (Int, String) Int)) + has (M.lookup [True] (M.empty :: M.Map [Bool] Int)) + has (M.lookup (Just False) (M.empty :: M.Map (Maybe Bool) Int)) + has (M.lookup (True, 3 :: Int) (M.empty :: M.Map (Bool, Int) Int)) + has (M.lookup [[4 :: Int]] (M.empty :: M.Map [[Int]] Int)) + has (M.lookup (Just (Just (5 :: Int))) (M.empty :: M.Map (Maybe (Maybe Int)) Int)) + has (M.lookup (6 :: Int, 7 :: Int, 8 :: Int) (M.empty :: M.Map (Int, Int, Int) Int)) + has (M.lookup [9.5 :: Number] (M.empty :: M.Map [Number] Int))

widenMember :: Int
widenMember = bit (M.member (1 :: Int) (M.empty :: M.Map Int Int)) + bit (M.member (1.5 :: Number) (M.empty :: M.Map Number Int)) + bit (M.member "s" (M.empty :: M.Map String Int)) + bit (M.member True (M.empty :: M.Map Bool Int)) + bit (M.member [1 :: Int] (M.empty :: M.Map [Int] Int)) + bit (M.member (Just (1 :: Int)) (M.empty :: M.Map (Maybe Int) Int)) + bit (M.member (1 :: Int, 2 :: Int) (M.empty :: M.Map (Int, Int) Int)) + bit (M.member ["a"] (M.empty :: M.Map [String] Int)) + bit (M.member (Just "b") (M.empty :: M.Map (Maybe String) Int)) + bit (M.member (1 :: Int, "c") (M.empty :: M.Map (Int, String) Int)) + bit (M.member [True] (M.empty :: M.Map [Bool] Int)) + bit (M.member (Just False) (M.empty :: M.Map (Maybe Bool) Int)) + bit (M.member (True, 3 :: Int) (M.empty :: M.Map (Bool, Int) Int)) + bit (M.member [[4 :: Int]] (M.empty :: M.Map [[Int]] Int)) + bit (M.member (Just (Just (5 :: Int))) (M.empty :: M.Map (Maybe (Maybe Int)) Int)) + bit (M.member (6 :: Int, 7 :: Int, 8 :: Int) (M.empty :: M.Map (Int, Int, Int) Int)) + bit (M.member [9.5 :: Number] (M.empty :: M.Map [Number] Int))

widenDelete :: Int
widenDelete = M.size (M.delete (1 :: Int) (M.empty :: M.Map Int Int)) + M.size (M.delete (1.5 :: Number) (M.empty :: M.Map Number Int)) + M.size (M.delete "s" (M.empty :: M.Map String Int)) + M.size (M.delete True (M.empty :: M.Map Bool Int)) + M.size (M.delete [1 :: Int] (M.empty :: M.Map [Int] Int)) + M.size (M.delete (Just (1 :: Int)) (M.empty :: M.Map (Maybe Int) Int)) + M.size (M.delete (1 :: Int, 2 :: Int) (M.empty :: M.Map (Int, Int) Int)) + M.size (M.delete ["a"] (M.empty :: M.Map [String] Int)) + M.size (M.delete (Just "b") (M.empty :: M.Map (Maybe String) Int)) + M.size (M.delete (1 :: Int, "c") (M.empty :: M.Map (Int, String) Int)) + M.size (M.delete [True] (M.empty :: M.Map [Bool] Int)) + M.size (M.delete (Just False) (M.empty :: M.Map (Maybe Bool) Int)) + M.size (M.delete (True, 3 :: Int) (M.empty :: M.Map (Bool, Int) Int)) + M.size (M.delete [[4 :: Int]] (M.empty :: M.Map [[Int]] Int)) + M.size (M.delete (Just (Just (5 :: Int))) (M.empty :: M.Map (Maybe (Maybe Int)) Int)) + M.size (M.delete (6 :: Int, 7 :: Int, 8 :: Int) (M.empty :: M.Map (Int, Int, Int) Int)) + M.size (M.delete [9.5 :: Number] (M.empty :: M.Map [Number] Int))

main :: IO ()
main = do
    print (widenLookup, widenMember, widenDelete)
    let mi = M.insert (3 :: Int) "c" (M.fromList [(1, "a"), (2, "b")])
    print (M.lookup 1 mi, M.lookup 4 mi, M.member 3 mi, M.toList (M.delete 2 mi))
    let ms = M.singleton "k" (1 :: Int)
    print (M.lookup "k" ms, M.member "j" ms, M.size (M.delete "k" ms))
    let mb = M.fromList [(False, "f"), (True, "t")]
    print (M.lookup True mb, M.member False mb, M.keys (M.delete False mb))
    let mn = M.fromList [(1.5 :: Number, "a"), (2.5, "b")]
    print (M.lookup 2.5 mn, M.lookup 3.5 mn, M.member 1.5 mn)
    let ml = M.fromList [([1, 2 :: Int], "onetwo"), ([], "nil"), ([2], "two")]
    print (M.lookup [1, 2] ml, M.lookup [] ml, M.lookup [1] ml, M.member [2] ml, M.toList (M.delete [] ml))
    let mz = M.insert (map (* 2) [1, 2 :: Int]) "even" (M.singleton [1 .. 3] "range")
    print (M.lookup ([2] ++ [4]) mz, M.lookup (take 3 [1 ..]) mz, M.member (map (+ 1) [1, 2]) mz, M.size (M.delete [2, 4] mz))
    let mll = M.fromList [([[1 :: Int], [2]], "a"), ([[]], "b")]
    print (M.lookup [[1], [2]] mll, M.lookup [[]] mll, M.lookup [] mll, M.member [[1]] mll)
    let mm = M.fromList [(Just (1 :: Int), "one"), (Nothing, "none")]
    print (M.lookup (Just 1) mm, M.lookup Nothing mm, M.member (Just 2) mm, M.keys (M.delete Nothing mm))
    let mmm = M.insert (Just (Just (1 :: Int))) "jj" (M.fromList [(Just Nothing, "jn"), (Nothing, "n")])
    print (M.lookup (Just (Just 1)) mmm, M.lookup (Just Nothing) mmm, M.member Nothing mmm, M.size (M.delete (Just Nothing) mmm))
    let mt = M.fromList [((1 :: Int, Just (2 :: Int)), "a"), ((1, Nothing), "b")]
    print (M.lookup (1, Just 2) mt, M.lookup (1, Nothing) mt, M.member (0, Nothing) mt, M.toList (M.delete (1, Nothing) mt))
    let m3 = M.singleton (1 :: Int, 2 :: Int, 3 :: Int) "123"
    print (M.lookup (1, 2, 3) m3, M.member (3, 2, 1) m3, M.size (M.delete (1, 2, 3) m3))
    let mss = M.fromList [(["a,b", "c"], 1 :: Int), (["a", "b,c"], 2)]
    print (M.lookup ["a", "b,c"] mss, M.lookup ["a,b", "c"] mss, M.member ["a,b,c"] mss)
