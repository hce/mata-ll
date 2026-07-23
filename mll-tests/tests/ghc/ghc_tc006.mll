-- GHC tc006: Higher-rank-ish: passing polymorphic functions as arguments
-- Tests passing functions that work on multiple types

applyTwice :: (a -> a) -> a -> a
applyTwice f x = f (f x)

applyBoth :: (a -> b) -> a -> a -> (b, b)
applyBoth f x y = (f x, f y)

on :: (b -> b -> c) -> (a -> b) -> a -> a -> c
on op f x y = op (f x) (f y)

myNegate :: Int -> Int
myNegate x = 0 - x

double :: Int -> Int
double x = x * 2

square :: Int -> Int
square x = x * x

applyList :: (a -> b) -> [a] -> [b]
applyList _ []     = []
applyList f (x:xs) = f x : applyList f xs

main :: IO ()
main = do
    assert (applyTwice double 3 == 12) "applyTwice double"
    assert (applyTwice myNegate 5 == 5) "applyTwice negate"
    assert (applyTwice square 2 == 16) "applyTwice square"

    let ab = applyBoth double 3 7
    assert (fst ab == 6) "applyBoth fst"
    assert (snd ab == 14) "applyBoth snd"

    -- on: compare by square
    assert (on (\a b -> a == b) square 3 3 == True) "on eq"
    assert (on (\a b -> a == b) square 2 2 == True) "on eq2"
    assert (on (\a b -> a < b) double 1 5 == True) "on lt"

    -- passing applyList itself a higher-order arg
    let ns = applyList double [1, 2, 3, 4]
    assert (ns == [2, 4, 6, 8]) "applyList double"
    let ms = applyList square [1, 2, 3, 4]
    assert (ms == [1, 4, 9, 16]) "applyList square"

    putStrLn "ok"
