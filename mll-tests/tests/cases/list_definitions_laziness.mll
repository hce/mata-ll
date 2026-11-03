-- scanl, scanl1, intersperse, span, break and the list instance's foldl'
-- are GHC's definitions clause for clause, so they demand exactly what
-- GHC demands: scanl emits the seed before it inspects the list (`take 1
-- (scanl (+) 0 undefined)` is `[0]`, the knot `ps = scanl (+) 1 ps` ties),
-- intersperse emits the first element before it inspects the tail, span
-- matches the list and decides `p x` BEFORE the pair exists (`span p
-- undefined` and `span (\_ -> undefined) [1]` raise) and runs one pass,
-- and foldl' forces the INCOMING accumulator at every cell (GHC.List: the
-- seed is forced on a non-empty list, `foldl' (\_ x -> x) undefined [1]`
-- raises), as does the class default (Data.Foldable's `z `seq` k (f z
-- x)`), which Maybe, Either and a foldr-only instance follow.
-- Before: scanl/scanl1 matched the list first, intersperse's `[x]` clause
-- matched the tail first, span was a pair of takeWhile/dropWhile (defined
-- on an undefined list, two traversals), foldl' forced only the result of
-- each application (the seed never). Also: element laziness, the sizes
-- (3·10^5 for span, 10^5 for scanl, 10^6 for foldl'), and the controls
-- that were already GHC's (unzip/unzip3, scanr/scanr1, partition, the
-- infinite-list prefixes). Partial values are shown with the whole string
-- forced first, so the GHC twin's output through `catch` is all-or-nothing
-- like mata-ll's.

import Data.List

data Box a = Full a | Vacant

instance Foldable Box where
    foldr _ z Vacant = z
    foldr f z (Full x) = f x z

shown :: Show a => Int -> a -> IO ()
shown n x = catch (let s = show x in seq (s == s) (putStrLn (show n <> " " <> s))) (\e -> putStrLn (show n <> " caught: " <> e))

total :: Foldable t => t Int -> Int
total t = foldl' (+) 0 t

lastOf :: Foldable t => Int -> t Int -> Int
lastOf z t = foldl' (\_ x -> x) z t

bottom :: [Int]
bottom = undefined

main :: IO ()
main = do
    shown 1 (take 1 (scanl (+) 0 bottom))
    shown 2 (take 2 (scanl (+) 0 (1 : bottom)))
    shown 3 (take 3 (scanl (+) 0 (1 : 2 : bottom)))
    shown 4 (take 1 (scanl1 (+) (1 : bottom)))
    shown 5 (take 1 (scanl1 (+) bottom))
    shown 6 (let ps = scanl (+) 1 ps in take 4 (ps :: [Int]))
    shown 7 (take 3 (scanl (+) 0 [1 :: Int ..]), take 3 (scanl1 (+) [1 :: Int ..]))
    shown 8 (length (scanl (\_ x -> x) 0 [undefined, 2 :: Int]), length (scanl1 (\_ x -> x) [1, undefined, 3 :: Int]))
    shown 9 (scanl (+) 0 [1, 2, 3 :: Int], scanl1 (+) [1, 2, 3 :: Int], scanl (+) 0 ([] :: [Int]), scanl1 (+) ([] :: [Int]))
    shown 10 (length (scanl (+) 0 [1 .. 100000 :: Int]), sum (scanl (+) 0 [1 .. 100000 :: Int]), last (scanl (+) 0 [1 .. 3000 :: Int]), length (scanl1 max [1 .. 100000 :: Int]))
    shown 11 (take 1 (intersperse 0 (1 : bottom)))
    shown 12 (take 2 (intersperse 0 (1 : bottom)))
    shown 13 (take 3 (intersperse 0 (1 : 2 : bottom)))
    shown 14 (null (intersperse 0 bottom))
    shown 15 (take 3 (intersperse 0 [1 :: Int ..]), length (intersperse 0 [1, undefined, 3 :: Int]))
    shown 16 (intersperse 0 [1, 2, 3 :: Int], intersperse 0 [1 :: Int], intersperse 0 ([] :: [Int]), intercalate [0] [[1, 2], [3 :: Int]])
    shown 17 (case span even bottom of (_, _) -> 1 :: Int)
    shown 18 (case span (\_ -> undefined) [1 :: Int] of (_, _) -> 1 :: Int)
    shown 19 (case span even [1 :: Int] of (_, _) -> 1 :: Int, case span even [] of (a, b) -> (a :: [Int], b))
    shown 20 (case break (\_ -> undefined) [1 :: Int] of (_, _) -> 1 :: Int)
    shown 21 (take 2 (fst (span even (2 : 4 : bottom))), take 2 (fst (break (> 4) (2 : 4 : bottom))))
    shown 22 (take 1 (snd (span even (1 : bottom))), take 1 (snd (break odd (1 : bottom))))
    shown 23 (take 2 (snd (span even (2 : 4 : bottom))))
    shown 24 (length (fst (span (\_ -> True) [undefined :: Int])), length (snd (span (\_ -> False) [undefined :: Int])))
    shown 25 (length (fst (span (\_ -> True) [undefined :: Int, undefined])), length (snd (break (\_ -> True) [undefined :: Int, undefined])))
    shown 26 (span even [2, 4, 5, 6 :: Int], break (> 3) [1 .. 6 :: Int], span even ([] :: [Int]), span even [1, 2 :: Int])
    shown 27 (take 2 (snd (span (< 3) [1 :: Int ..])), take 2 (fst (break (> 3) [1 :: Int ..])), take 2 (fst (span even [2, 4 :: Int ..])))
    shown 28 (length (fst (span (< 300001) [1 .. 300000 :: Int])), length (snd (span even [1 .. 300000 :: Int])))
    shown 29 (length (snd (span (< 300000) [1 .. 600000 :: Int])), length (fst (break (> 300000) [1 .. 600000 :: Int])))
    shown 30 (foldl' (\_ x -> x) (undefined :: Int) [1])
    shown 31 (lastOf undefined [1 :: Int, 2])
    shown 32 (length [foldl' (+) (undefined :: Int) []], length [lastOf undefined ([] :: [Int])])
    shown 33 (foldl' (\a _ -> a) (0 :: Int) [undefined], foldl' (\a _ -> a) (0 :: Int) [undefined, undefined])
    shown 34 (foldl' (+) (0 :: Int) [1, undefined])
    shown 35 (foldl' (\_ x -> x) (undefined :: Int) (Just 1))
    shown 36 (foldl' (\_ x -> x) (undefined :: Int) (Right 1 :: Either String Int))
    shown 46 (foldl' (\_ x -> x) (undefined :: Int) (Full 1))
    shown 47 (lastOf undefined (Full 2))
    shown 48 (lastOf undefined (Just 3))
    shown 49 (foldl' (\a _ -> a) (0 :: Int) (Just undefined), foldl' (\a _ -> a) (0 :: Int) (Full undefined), foldl' (\a _ -> a) (0 :: Int) (Right undefined :: Either String Int))
    shown 50 (foldl' (\_ x -> x) (0 :: Int) (Just 1), foldl' (\_ x -> x) (0 :: Int) (Full 2), lastOf 0 (Right 3 :: Either String Int), lastOf 0 (Just 4), lastOf 0 (Full 5))
    shown 37 (foldl' (+) (undefined :: Int) Nothing, lastOf undefined Vacant, lastOf undefined (Left "x" :: Either String Int))
    shown 38 (foldl' (+) 0 [1 .. 1000000 :: Int], total [1 .. 1000000], total (Just 4), total (Full 5), total Vacant)
    shown 39 (sum (map (* 2) (filter odd [1 .. 1000 :: Int])), length (filter even [1 .. 1000 :: Int]), product [1 .. 10 :: Int])
    shown 40 (foldl' (flip (:)) [] [1, 2, 3 :: Int], foldl' (\acc x -> acc <> show x) "" [1, 2, 3 :: Int])
    shown 41 (take 3 (fst (unzip (zip [1 :: Int ..] [1 :: Int ..]))), (case unzip3 (zip3 [1 :: Int ..] [1 :: Int ..] [1 :: Int ..]) of (_, b, _) -> take 3 b))
    shown 42 (length (fst (unzip (zip [1 .. 300000 :: Int] [1 .. 300000 :: Int]))), take 3 (fst (unzip ((1, 2) : (3, 4) : (5, 6) : (undefined :: [(Int, Int)])))))
    shown 43 (length (fst (unzip [undefined :: (Int, Int)])))
    shown 44 (take 1 (scanr (\x _ -> x) 0 [1 :: Int ..]), take 2 (scanr1 (\x _ -> x) [1 :: Int ..]), take 2 (scanr (+) 0 (1 : 2 : bottom)))
    shown 45 (take 3 (fst (partition even [1 :: Int ..])), take 3 (fst (partition even (2 : 4 : 6 : bottom))))
