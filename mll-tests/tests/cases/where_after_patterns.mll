-- A clause's where group is emitted once, after the clause's pattern
-- bindings. A single-clause function with a destructuring pattern used to
-- emit its where group twice — once before any pattern variable was a Lua
-- local (an eager where value read the pattern variables as nil globals)
-- and once after — so `f (a, b) = a + b + c where c = a * 10` raised
-- "arithmetic on a nil value". Every destructuring shape (tuple,
-- constructor, record, list, nested, as-pattern) with a strict, lazy,
-- exact-first-force and shadowing where value, a where function reading a
-- pattern variable and a mutually recursive pair; the same with guards,
-- eta padding, a nested group, an IO body and a CAF. Found while fixing
-- the 2026-10 review's L5.
-- expect: 13
-- expect: 5
-- expect: 102
-- expect: 12
-- expect: 7
-- expect: 15
-- expect: False
-- expect: True
-- expect: 15
-- expect: 12
-- expect: 15
-- expect: 7
-- expect: 12
-- expect: 10
-- expect: 0
-- expect: 13
-- expect: 200
-- expect: 5
-- expect: 14
-- expect: 3
-- expect: 4
-- expect: 6
-- expect: 31
-- expect: 41
-- expect: 4
-- expect: 0
-- expect: 16
-- expect: 13
-- expect: 10
-- expect: 2012
-- expect: 6
-- expect: 7
-- expect: 3x4=12
-- expect: 42
module Main where

data Box = Box Int Int
data P = P { px :: Int, py :: Int }
data T = T Int (Int, Int)

tupStrict :: (Int, Int) -> Int
tupStrict (a, b) = a + b + c
  where
    c = a * 10

tupLazy :: (Int, Int) -> Int
tupLazy (a, b) = if a > 0 then c else b
  where
    c = error "tupLazy forced c"

tupLazyUsed :: (Int, Int) -> Int
tupLazyUsed (a, b) = if a > 0 then c else b
  where
    c = a * 100 + b

tupExact :: ([Int], Int) -> Int
tupExact (xs, k) = case total of
    0 -> k
    n -> n * k
  where
    total = sum xs

tupFn :: (Int, Int) -> Int
tupFn (a, b) = scale b + scale 1
  where
    scale y = y * a

tupMutual :: (Int, Int) -> Bool
tupMutual (a, b) = ev (a + b)
  where
    ev 0 = True
    ev n = od (n - 1)
    od 0 = False
    od n = ev (n - 1)

tupShadow :: (Int, Int) -> Int
tupShadow (a, b) = a + b
  where
    a = b * 2

tupShadowLazy :: (Int, Int) -> Int
tupShadowLazy (a, b) = if b > 0 then a else 0
  where
    a = sum [b, b, b]

boxStrict :: Box -> Int
boxStrict (Box w h) = area + w
  where
    area = w * h

boxFn :: Box -> Int
boxFn (Box w h) = grow h
  where
    grow k = k + w

recStrict :: P -> Int
recStrict (P x y) = d + x
  where
    d = x * y

recLazy :: P -> Int
recLazy whole@(P x _) = if x > 0 then d else px whole
  where
    d = x * py whole

listStrict :: [Int] -> Int
listStrict (x : rest) = x + n
  where
    n = length rest
listStrict [] = 0

listLazy :: [Int] -> Int
listLazy (x : rest) = if x > 0 then x else n
  where
    n = length rest * 100
listLazy [] = 0

nestedStrict :: T -> Int
nestedStrict (T n (a, b)) = n * s
  where
    s = a + b

nestedFn :: T -> Int
nestedFn (T n (a, b)) = pick n
  where
    pick 0 = a
    pick _ = b

asStrict :: (Int, Int) -> Int
asStrict whole@(a, b) = fst whole + a + c
  where
    c = b * 2

tupGuard :: (Int, Int) -> Int
tupGuard (a, b)
    | a > b = big
    | otherwise = small
  where
    big = a * 10 + b
    small = b * 10 + a

boxGuardFn :: Box -> Int
boxGuardFn (Box w h)
    | w == 0 = 0
    | otherwise = per h
  where
    per k = k `div` w

tupEta :: (Int, Int) -> Int -> Int
tupEta (a, b) = \x -> x + c
  where
    c = a * b

tupEtaFn :: (Int, Int) -> Int -> Int
tupEtaFn (a, b) = add
  where
    add x = x + a - b

boxEta :: Box -> Int -> Int
boxEta (Box w h) = (+) area
  where
    area = w * h

nestedGroup :: (Int, [Int]) -> Int
nestedGroup (k, xs) = go xs
  where
    go [] = base
    go (y : ys) = let d = y * k in d + go ys
    base = k * 1000

ioTup :: (Int, Int) -> IO ()
ioTup (a, b) = do
    print c
    print (c + b)
  where
    c = a * 3

ioBox :: Box -> IO ()
ioBox (Box w h) = putStrLn (show w <> "x" <> show h <> "=" <> show area)
  where
    area = w * h

cafWhere :: Int
cafWhere = c + 1
  where
    c = 41

main :: IO ()
main = do
    print (tupStrict (1, 2))
    print (tupLazy (0, 5))
    print (tupLazyUsed (1, 2))
    print (tupExact ([1, 2, 3], 2))
    print (tupExact ([], 7))
    print (tupFn (3, 4))
    print (tupMutual (3, 4))
    print (tupMutual (2, 4))
    print (tupShadow (1, 5))
    print (tupShadowLazy (1, 4))
    print (boxStrict (Box 3 4))
    print (boxFn (Box 3 4))
    print (recStrict (P 2 5))
    print (recLazy (P 2 5))
    print (recLazy (P 0 5))
    print (listStrict [10, 1, 2, 3])
    print (listLazy [-1, 1, 2])
    print (listLazy [5])
    print (nestedStrict (T 2 (3, 4)))
    print (nestedFn (T 0 (3, 4)))
    print (nestedFn (T 1 (3, 4)))
    print (asStrict (1, 2))
    print (tupGuard (3, 1))
    print (tupGuard (1, 4))
    print (boxGuardFn (Box 2 9))
    print (boxGuardFn (Box 0 9))
    print (tupEta (2, 3) 10)
    print (tupEtaFn (5, 2) 10)
    print (boxEta (Box 2 3) 4)
    print (nestedGroup (2, [1, 2, 3]))
    ioTup (2, 1)
    ioBox (Box 3 4)
    print cafWhere
