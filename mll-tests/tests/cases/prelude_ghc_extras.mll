-- The GHC Prelude functions added on 2026-09-12 after the PSE dogfood found
-- `either`/`maybe` missing: either, maybe, curry, uncurry, asTypeOf, until,
-- ($!), (=<<), (<$), sequence_, gcd, lcm, splitAt, break, scanl/scanl1/
-- scanr/scanr1, foldr1/foldl1, zip3/zipWith3/unzip3, notElem — each at the
-- shapes GHC gives them (Integral-generic gcd/lcm at Int and Integer,
-- Foldable-generic foldr1/foldl1/sequence_/notElem over Maybe as well as
-- lists, laziness of scanl/until over infinite input).

halve :: Int -> Either String Int
halve n = if even n then Right (n `div` 2) else Left ("odd: " <> show n)

main :: IO ()
main = do
    print (either (const (-1)) (* 2) (Left "abc" :: Either String Int), either (const (-1)) (* 2) (Right 21 :: Either String Int))
    print (map (either (const 0) id) [halve 10, halve 7, halve 0])
    print (maybe 0 (+ 1) (Just 41), maybe 0 (+ 1) Nothing)
    print (curry fst 1 2 :: Int, uncurry (+) (3, 4) :: Int, uncurry max (2, 9) :: Int)
    print (asTypeOf 3 (7 :: Int), asTypeOf 2.5 (1 :: Number))
    print (until (> 1000) (* 2) 1 :: Int, until null tail [1, 2, 3 :: Int])
    print (succ $! (41 :: Int), length $! [1, 2, 3 :: Int])
    print (halve =<< halve 20, halve =<< halve 6, (\x -> [x, x]) =<< [1, 2 :: Int])
    print (7 <$ Just "x", "a" <$ [1, 2, 3 :: Int], True <$ (Right 0 :: Either String Int))
    sequence_ [putStrLn "one", putStrLn "two"]
    sequence_ (Just (putStrLn "three"))
    sequence_ (Nothing :: Maybe (IO ()))
    print (sequence_ [Right 1, Left "stop", Left "later" :: Either String Int])
    print (gcd 12 18 :: Int, gcd (-12) 18 :: Int, gcd 0 5 :: Int, gcd 0 0 :: Int, gcd 7 0 :: Int)
    print (lcm 4 6 :: Int, lcm (-4) 6 :: Int, lcm 0 9 :: Int, lcm 9 0 :: Int)
    print (gcd (2 ^ 70) (2 ^ 65 * 3), lcm (2 ^ 62) (3 ^ 40))
    print (splitAt 2 [1, 2, 3, 4, 5 :: Int], splitAt 0 [1 :: Int], splitAt 9 [1, 2 :: Int], splitAt (-1) [1, 2 :: Int])
    print (break (> 3) [1, 2, 3, 4, 5, 1 :: Int], break even [1, 3, 5 :: Int], break odd ([] :: [Int]))
    print (scanl (+) 0 [1, 2, 3, 4 :: Int], scanl (flip (:)) [] [1, 2, 3 :: Int])
    print (scanl1 (+) [1, 2, 3, 4 :: Int], scanl1 max ([] :: [Int]))
    print (scanr (+) 0 [1, 2, 3, 4 :: Int], scanr (:) [] [1, 2, 3 :: Int])
    print (scanr1 (+) [1, 2, 3, 4 :: Int], scanr1 min [9 :: Int], scanr1 min ([] :: [Int]))
    print (take 5 (scanl (+) 0 [1 ..]) :: [Integer], take 3 (scanl1 (*) [2 ..]) :: [Integer])
    print (foldr1 (-) [10, 3, 2 :: Int], foldl1 (-) [10, 3, 2 :: Int], foldr1 max [4, 9, 1 :: Int])
    print (foldr1 (+) (Just (5 :: Int)), foldl1 (+) (Right 6 :: Either String Int))
    print (zip3 [1, 2, 3 :: Int] ["a", "b", "c", "d"] [True, False], zipWith3 (\a b c -> a + b * c) [1, 2 :: Int] [3, 4] [5, 6, 7])
    print (unzip3 [(1 :: Int, "x", True), (2, "y", False)], unzip3 ([] :: [(Int, Int, Int)]))
    print (notElem 3 [1, 2, 3 :: Int], notElem 4 [1, 2, 3 :: Int], notElem 1 (Just (1 :: Int)), notElem 1 (Nothing :: Maybe Int))
