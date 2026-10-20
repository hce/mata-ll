-- W24 (H2, H3): Data.Set over the same machinery. `widen` uses S.insert
-- at 17 element types, past the specialization limit, so every S.insert
-- below runs the dictionary-form body — the element encoder read off the
-- Hashable dictionary the call site builds — while S.member, S.delete,
-- S.fromList and S.toList stay specialized; union/intersection/
-- difference mix both.
module Main where

import qualified Data.Set as S

widen :: Int
widen = S.size (S.insert (1 :: Int) S.empty) + S.size (S.insert (1.5 :: Number) S.empty) + S.size (S.insert "s" S.empty) + S.size (S.insert True S.empty) + S.size (S.insert [1 :: Int] S.empty) + S.size (S.insert (Just (1 :: Int)) S.empty) + S.size (S.insert (1 :: Int, 2 :: Int) S.empty) + S.size (S.insert ["a"] S.empty) + S.size (S.insert (Just "b") S.empty) + S.size (S.insert (1 :: Int, "c") S.empty) + S.size (S.insert [True] S.empty) + S.size (S.insert (Just False) S.empty) + S.size (S.insert (True, 3 :: Int) S.empty) + S.size (S.insert [[4 :: Int]] S.empty) + S.size (S.insert (Just (Just (5 :: Int))) S.empty) + S.size (S.insert (6 :: Int, 7 :: Int, 8 :: Int) S.empty) + S.size (S.insert [9.5 :: Number] S.empty)

main :: IO ()
main = do
    print widen
    let s = S.insert [2 :: Int] (S.insert [1, 2] (S.insert [] (S.insert [1] S.empty)))
    print (S.member [1, 2] s, S.member [2, 1] s, S.member [] s, S.size s, S.toList s)
    print (S.toList (S.delete [] s), S.member [] (S.delete [] s))
    let t = S.fromList [Just (3 :: Int), Nothing, Just 1]
    print (S.toList (S.insert (Just 2) t), S.member Nothing t, S.size (S.insert Nothing t))
    let u = S.union (S.fromList [(1 :: Int, "a"), (2, "b")]) (S.insert (2, "b") (S.insert (0, "z") S.empty))
    print (S.toList u, S.size u)
    let v = S.insert (map (+ 1) [0, 1 :: Int]) (S.fromList [[1 .. 2], [3] ++ [4]])
    print (S.toList v, S.member [1, 2] v, S.member (take 2 [3 ..]) v)
    let w = S.insert "b" (S.insert "a" S.empty)
    print (S.toList w, S.member "a" w, S.member "c" w)
    let x = S.insert True (S.insert False S.empty)
    print (S.toList x, S.toList (S.intersection x (S.fromList [True])), S.toList (S.difference x (S.fromList [True])))
    let y = S.insert (Just (Just (2 :: Int))) (S.insert (Just Nothing) (S.insert Nothing S.empty))
    print (S.toList y, S.member (Just Nothing) y, S.member (Just (Just 1)) y)
    let z = S.insert (1 :: Int, Just "q") (S.insert (0, Nothing) S.empty)
    print (S.toList z, S.member (1, Just "q") z, S.member (1, Nothing) z)
