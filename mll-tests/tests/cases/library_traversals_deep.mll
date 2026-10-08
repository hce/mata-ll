import Control.Monad
import Data.List
import qualified Data.Set as S
import qualified Data.Map as M

n :: Int
n = 1000000

ones :: [Int]
ones = replicate n 1

main :: IO ()
main = do
    xs <- mapM (\x -> pure (x + 1)) [1 .. n]
    assert (length xs == n && sum xs == 500001500000) "mapM in IO"
    ys <- sequence (map (\x -> pure x) [1 .. n])
    assert (length ys == n) "sequence in IO"
    zs <- traverse (\x -> pure (x * 2)) [1 .. n]
    assert (sum zs == 1000001000000) "traverse in IO"
    ws <- forM [1 .. n] (\x -> pure x)
    assert (length ws == n) "forM in IO"
    assert (fmap length (mapM (\x -> Just x) [1 .. n]) == Just n) "mapM in Maybe"
    assert (fmap length (mapM (\x -> if x > 0 then Right x else Left x) [1 .. n]) == (Right n :: Either Int Int)) "mapM in Either"
    assert (mapM (\x -> if x == n then Nothing else Just x) [1 .. n] == Nothing) "mapM in Maybe, failing last"
    assert (mapM_ (\x -> Just x) [1 .. n] == Just ()) "mapM_ in Maybe"
    assert (sequence_ (map (\x -> Right x) [1 .. n]) == (Right () :: Either Int ())) "sequence_ in Either"
    assert (forM_ [1 .. n] (\x -> Just x) == Just ()) "forM_ in Maybe"
    assert (not (elem 0 ones) && notElem 0 ones) "elem and notElem"
    assert (and (map (\x -> x == 1) ones) && not (or (map (\x -> x == 0) ones))) "and and or"
    assert (not (any (\x -> x > n) [1 .. n]) && all (\x -> x > 0) [1 .. n]) "any and all"
    assert (null (concat (replicate n [] :: [[Int]]))) "concat of empty fragments"
    assert (concatMap (\x -> if x == n then [x] else []) [1 .. n] == [n]) "concatMap of empty fragments"
    assert ([x | x <- [1 .. n], x == n] == [n]) "filtering comprehension"
    assert (foldMap (\x -> if x == n then [x] else []) [1 .. n] == [n]) "foldMap at lists"
    assert (mconcat (replicate n [] ++ [[n]]) == [n]) "mconcat at lists"
    assert (foldMap (\x -> if x == n then "end" else "") [1 .. n] == "end") "foldMap at String"
    assert (length (fst (unzip (zip [1 .. n] [1 .. n]))) == n) "unzip"
    assert (length (snd (unzip (zip [1 .. n] [1 .. n]))) == n) "unzip second"
    assert (length (fst (partition (\x -> x > 0) [1 .. n])) == n) "partition first"
    assert (length (snd (partition (\x -> x > 0) [1 .. n])) == 0) "partition second"
    assert (length (scanr (\x _ -> x) 0 [1 .. n]) == n + 1) "scanr spine"
    assert (length (scanr1 (\x _ -> x) [1 .. n]) == n) "scanr1 spine"
    assert (S.size (S.fromList [1 .. n]) == n) "Set.fromList"
    assert (S.size (S.union (S.fromList [1 .. n]) (S.fromList [0, 1])) == n + 1) "Set.union"
    assert (M.size (M.fromList (zip [1 .. n] ones)) == n) "Map.fromList"
