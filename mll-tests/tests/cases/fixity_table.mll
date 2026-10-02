infixl 6 `l6`
l6 :: Int -> Int -> Int
l6 a b = 2 * a - b

infixl 7 `l7`
l7 :: Int -> Int -> Int
l7 a b = 2 * a - b

infixl 7 `l7n`
l7n :: Number -> Number -> Number
l7n a b = 2 * a - b

infixr 8 `r8`
r8 :: Int -> Int -> Int
r8 a b = 2 * a - b

infixl 5 `l5`
l5 :: Int -> Int -> Int
l5 a b = 2 * a - b

infixr 5 `r5`
r5 :: [Int] -> [Int] -> [Int]
r5 a b = reverse (a ++ b)

infixl 9 `l9`
l9 :: [Int] -> Int -> [Int]
l9 xs n = drop n xs

infixr 9 `r9`
r9 :: [Int] -> (Int -> Int) -> [Int]
r9 xs f = map f xs

infixr 0 `r0`
r0 :: (Int -> Int) -> Int -> Int
r0 f x = f x - 1

infixl 1 `l1i`
l1i :: Int -> Int -> Int
l1i a b = 2 * a - b

infixr 3 `r3`
r3 :: Bool -> Bool -> Bool
r3 _ b = not b

infixr 2 `r2`
r2 :: Bool -> Bool -> Bool
r2 _ b = not b

infixr 3 `when3`
when3 :: Int -> Bool -> Int
when3 a b = if b then a else 0 - a

infixl 4 `l4`
l4 :: Maybe Int -> Int -> Maybe Int
l4 m n = fmap (\x -> 2 * x - n) m

infixl 5 `m5`
m5 :: Maybe Int -> Int -> Maybe Int
m5 m n = fmap (\x -> 2 * x - n) m

infixl 1 `l1`
l1 :: Maybe Int -> Int -> Maybe Int
l1 m n = Just (maybe n (\x -> 2 * x - n) m)

infixr 1 `r1`
r1 :: (Int -> Maybe Int) -> Maybe Int -> Maybe Int
r1 g m = fmap (* 2) (m >>= g)

infixr 6 `r6`
r6 :: String -> String -> String
r6 a b = b <> a

inc :: Int -> Maybe Int
inc x = Just (x + 1)

tenfold :: Int -> Maybe Int
tenfold x = Just (x * 10)

main :: IO ()
main = do
    assert ((3 + 4 `l6` 5, 3 `l6` 4 + 5) == (9, 7)) "+ is infixl 6"
    assert ((9 - 4 `l6` 5, 9 `l6` 4 - 5) == (5, 9)) "- is infixl 6"
    assert ((3 * 4 `l7` 5, 3 `l7` 4 * 5) == (19, 10)) "* is infixl 7"
    assert ((8 / 4 `l7n` 1, 8 `l7n` 4 / 2) == (3, 6)) "/ is infixl 7"
    assert ((20 `div` 3 `l7` 1, 20 `l7` 3 `div` 4) == (11, 9)) "div is infixl 7"
    assert ((20 `mod` 7 `l7` 1, 20 `l7` 3 `mod` 5) == (11, 2)) "mod is infixl 7"
    assert ((20 `quot` 3 `l7` 1, 20 `l7` 3 `quot` 4) == (11, 9)) "quot is infixl 7"
    assert ((20 `rem` 7 `l7` 1, 20 `l7` 3 `rem` 5) == (11, 2)) "rem is infixl 7"
    assert ((2 ^ 3 `r8` 2, 7 `r8` 2 ^ 3) == (16, 6)) "^ is infixr 8"
    assert (([1] ++ [2] `r5` [3], [1] `r5` [2] ++ [3]) == ([1, 3, 2], [3, 2, 1])) "++ is infixr 5"
    assert ((1 : [2] `r5` [3], [1] `r5` 2 : [3]) == ([1, 3, 2], [3, 2, 1])) ": is infixr 5"
    assert (([[1, 2], [3, 4]] !! 1 `l9` 1, [5, 6, 7, 8] `l9` 1 !! 1) == ([4], 7)) "!! is infixl 9"
    assert ([1, 2] `r9` (* 2) . (+ 1) == [4, 6]) ". is infixr 9"
    assert (((* 2) $ (+ 1) `r0` 5, (* 2) `r0` (+ 1) $ 5) == (10, 11)) "$ is infixr 0"
    assert (((* 2) $! (+ 1) `r0` 5, (* 2) `r0` (+ 1) $! 5) == (10, 11)) "$! is infixr 0"
    assert ((10 `l1i` 1 `seq` 7, (* 2) `r0` 1 `seq` 5) == (7, 9)) "seq is infixr 0"
    assert ((False && True `r3` False, False `r3` True && False) == (False, True)) "&& is infixr 3"
    assert ((True || False `r2` True, False `r2` False || True) == (True, False)) "|| is infixr 2"
    assert ((7 `when3` 1 == 1, 2 == 5 `l5` 8) == (7, True)) "== is at precedence 4"
    assert ((7 `when3` 1 /= 1, 2 /= 5 `l5` 8) == (-7, False)) "/= is at precedence 4"
    assert ((7 `when3` 1 < 2, 1 < 5 `l5` 8) == (7, True)) "< is at precedence 4"
    assert ((7 `when3` 2 <= 1, 2 <= 5 `l5` 8) == (-7, True)) "<= is at precedence 4"
    assert ((7 `when3` 2 > 1, 3 > 5 `l5` 8) == (7, True)) "> is at precedence 4"
    assert ((7 `when3` 1 >= 2, 1 >= 5 `l5` 8) == (-7, False)) ">= is at precedence 4"
    assert ((7 `when3` 1 `elem` [1], 2 `elem` [5] `r5` [2]) == (7, True)) "elem is at precedence 4"
    assert ((7 `when3` 1 `notElem` [1], 2 `notElem` [5] `r5` [2]) == (-7, False)) "notElem is at precedence 4"
    assert (((+ 1) <$> Just 3 `l4` 1, (+ 1) <$> Just 3 `m5` 1) == (Just 7, Just 6)) "<$> is infixl 4"
    assert ((7 <$ Just 3 `l4` 1, 7 <$ Just 3 `m5` 1) == (Just 13, Just 7)) "<$ is infixl 4"
    assert ((Just (+ 1) <*> Just 3 `l4` 1, Just (+ 1) <*> Just 3 `m5` 1) == (Just 7, Just 6)) "<*> is infixl 4"
    assert ((Just 3 >>= inc `l1` 1, Just 3 `l1` 1 >>= inc) == (Just 7, Just 6)) ">>= is infixl 1"
    assert ((Nothing >> Just 4 `l1` 1, Just 3 `l1` 1 >> Just 9) == (Just 1, Just 9)) ">> is infixl 1"
    assert ((inc =<< tenfold `r1` Just 3, inc `r1` tenfold =<< Just 3) == (Just 61, Just 62)) "=<< is infixr 1"
    assert (("a" <> "b" `r6` "c", "a" `r6` "b" <> "c") == ("acb", "bca")) "<> is infixr 6"
    print (3 + 4 `l6` 5, 3 `l6` 4 + 5, 9 - 4 `l6` 5, 9 `l6` 4 - 5)
    print (3 * 4 `l7` 5, 3 `l7` 4 * 5, 8 / 4 `l7n` 1, 8 `l7n` 4 / 2)
    print (20 `div` 3 `l7` 1, 20 `l7` 3 `div` 4, 20 `mod` 7 `l7` 1, 20 `l7` 3 `mod` 5)
    print (20 `quot` 3 `l7` 1, 20 `l7` 3 `quot` 4, 20 `rem` 7 `l7` 1, 20 `l7` 3 `rem` 5)
    print (2 ^ 3 `r8` 2, 7 `r8` 2 ^ 3)
    print ([1] ++ [2] `r5` [3], [1] `r5` [2] ++ [3], 1 : [2] `r5` [3], [1] `r5` 2 : [3])
    print ([[1, 2], [3, 4]] !! 1 `l9` 1, [5, 6, 7, 8] `l9` 1 !! 1)
    print ([1, 2] `r9` (* 2) . (+ 1))
    print ((* 2) $ (+ 1) `r0` 5, (* 2) `r0` (+ 1) $ 5, (* 2) $! (+ 1) `r0` 5, (* 2) `r0` (+ 1) $! 5)
    print (10 `l1i` 1 `seq` 7, (* 2) `r0` 1 `seq` 5)
    print (False && True `r3` False, False `r3` True && False, True || False `r2` True, False `r2` False || True)
    print (7 `when3` 1 == 1, 2 == 5 `l5` 8, 7 `when3` 1 /= 1, 2 /= 5 `l5` 8)
    print (7 `when3` 1 < 2, 1 < 5 `l5` 8, 7 `when3` 2 <= 1, 2 <= 5 `l5` 8)
    print (7 `when3` 2 > 1, 3 > 5 `l5` 8, 7 `when3` 1 >= 2, 1 >= 5 `l5` 8)
    print (7 `when3` 1 `elem` [1], 2 `elem` [5] `r5` [2], 7 `when3` 1 `notElem` [1], 2 `notElem` [5] `r5` [2])
    print ((+ 1) <$> Just 3 `l4` 1, (+ 1) <$> Just 3 `m5` 1, 7 <$ Just 3 `l4` 1, 7 <$ Just 3 `m5` 1)
    print (Just (+ 1) <*> Just 3 `l4` 1, Just (+ 1) <*> Just 3 `m5` 1)
    print (Just 3 >>= inc `l1` 1, Just 3 `l1` 1 >>= inc, Nothing >> Just 4 `l1` 1, Just 3 `l1` 1 >> Just 9)
    print (inc =<< tenfold `r1` Just 3, inc `r1` tenfold =<< Just 3)
    putStrLn ("a" <> "b" `r6` "c" <> " " <> "a" `r6` "b" <> "c")
