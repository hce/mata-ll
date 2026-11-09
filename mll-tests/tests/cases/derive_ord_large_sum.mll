-- W66 (O10, M15): a derived `Ord`/`Eq` of a 40-constructor sum type with
-- fields of ten shapes (nullary, one to three scalars, a list, a Maybe, a
-- tuple, two nested derived types and the type itself). H2010 §11.1: a
-- pair of different constructors is decided by declaration position
-- whatever the fields say; a pair of the same constructor compares the
-- fields left to right with each field type's own compare. The derived
-- methods used to be one clause per constructor PAIR (quadratic text,
-- refused by LuaJIT from 66 constructors) and are now one clause per
-- constructor plus one index comparison. Pinned against GHC on
-- first/last/adjacent pairs, every field position of the multi-field
-- shapes, the nested types' own orders, recursion, sort/maximum/minimum,
-- max/min, and the same through an Ord dictionary (`relD` is used at more
-- types than the specialization limit; `relHidden` through an existential).
module Main where

data Leaf = Leaf Int String
    deriving (Eq, Ord, Show)

data Tag = Alpha | Beta | Gamma
    deriving (Eq, Ord, Show, Enum, Bounded)

data Wide
    = W00 Int | W01 Int String | W02 | W03 Leaf | W04 Tag Int
    | W05 [Int] | W06 (Maybe Int) | W07 Int Int Int | W08 (Int, String) | W09 Wide
    | W10 Int | W11 Int String | W12 | W13 Leaf | W14 Tag Int
    | W15 [Int] | W16 (Maybe Int) | W17 Int Int Int | W18 (Int, String) | W19 Wide
    | W20 Int | W21 Int String | W22 | W23 Leaf | W24 Tag Int
    | W25 [Int] | W26 (Maybe Int) | W27 Int Int Int | W28 (Int, String) | W29 Wide
    | W30 Int | W31 Int String | W32 | W33 Leaf | W34 Tag Int
    | W35 [Int] | W36 (Maybe Int) | W37 Int Int Int | W38 (Int, String) | W39 Wide
    deriving (Eq, Ord, Show)

data Hidden = forall a. (Ord a, Show a) => Hidden a a

rel :: Ord a => a -> a -> String
rel x y = show (compare x y) <> " " <> show [x == y, x /= y, x < y, x <= y, x > y, x >= y, max x y == y, min x y == x]

relD :: Ord a => a -> a -> String
relD x y = show (compare x y) <> " " <> show [x == y, x /= y, x < y, x <= y, x > y, x >= y, max x y == y, min x y == x]

relHidden :: Hidden -> String
relHidden (Hidden x y) = relD x y <> " " <> show (max x y, min x y)

widen :: [String]
widen =
    [ relD (1 :: Int) 2, relD (2.5 :: Number) 1.5, relD "a" "b", relD True False, relD () ()
    , relD LT GT, relD [1 :: Int] [], relD (Just (1 :: Int)) Nothing, relD (1 :: Int, "a") (1, "b")
    , relD ["a"] ["a"], relD (Just "x") (Just "y"), relD (1 :: Int, 2 :: Int, 3 :: Int) (1, 2, 4)
    , relD [[1 :: Int]] [[2]], relD (Just (Just True)) (Just Nothing), relD (True, "t") (False, "t")
    , relD [2.5 :: Number] [1.5], relD (Just (1 :: Int, 2 :: Int)) Nothing, relD [True] [False]
    , relD (Left 1 :: Either Int String) (Right "r")
    ]

sample :: [Wide]
sample =
    [ W39 (W00 1), W00 7, W12, W01 1 "b", W01 1 "a", W23 (Leaf 2 "x"), W23 (Leaf 2 "w"), W14 Gamma 0, W14 Alpha 9
    , W35 [1, 2, 3], W35 [1, 2], W35 [], W26 Nothing, W26 (Just 0), W17 1 2 3, W17 1 2 2, W17 0 9 9, W38 (2, "a"), W38 (1, "z")
    , W39 (W39 (W00 2)), W39 (W39 (W00 1)), W39 W02, W00 7, W02, W22, W32
    ]

main :: IO ()
main = do
    mapM_ putStrLn
        [ rel (W00 999) (W01 0 ""), rel (W01 0 "") (W00 999), rel (W00 1) (W00 2), rel (W00 2) (W00 1), rel (W00 5) (W00 5)
        , rel (W01 1 "b") (W01 2 "a"), rel (W01 1 "b") (W01 1 "a"), rel (W01 1 "a") (W01 1 "ab"), rel (W01 3 "z") (W01 3 "z")
        , rel W02 (W01 9 "z"), rel W02 (W03 (Leaf 0 "")), rel W02 W02, rel W02 W12, rel W22 W12
        , rel (W03 (Leaf 1 "b")) (W03 (Leaf 1 "a")), rel (W03 (Leaf 0 "z")) (W03 (Leaf 1 "a")), rel (W03 (Leaf 1 "a")) (W03 (Leaf 1 "a"))
        , rel (W04 Gamma 0) (W04 Alpha 9), rel (W04 Beta 1) (W04 Beta 2), rel (W04 Beta 2) (W04 Beta 2)
        , rel (W05 [1, 2]) (W05 [1, 2, 3]), rel (W05 [2]) (W05 [1, 9]), rel (W05 []) (W05 [0]), rel (W05 []) (W05 [])
        , rel (W06 Nothing) (W06 (Just 0)), rel (W06 (Just 1)) (W06 (Just 0)), rel (W06 Nothing) (W06 Nothing)
        , rel (W07 1 2 3) (W07 1 2 4), rel (W07 1 3 0) (W07 1 2 9), rel (W07 2 0 0) (W07 1 9 9), rel (W07 1 2 3) (W07 1 2 3)
        , rel (W08 (1, "b")) (W08 (1, "a")), rel (W08 (0, "z")) (W08 (1, "a")), rel (W08 (1, "a")) (W08 (1, "a"))
        , rel (W09 (W00 1)) (W09 (W00 2)), rel (W09 (W39 W02)) (W09 (W39 (W00 0))), rel (W09 W02) (W09 (W01 0 "")), rel (W09 W12) (W09 W12)
        , rel (W39 (W00 0)) (W38 (9, "z")), rel (W00 0) (W39 (W00 0)), rel (W39 (W39 (W00 1))) (W39 (W39 (W00 2)))
        ]
    mapM_ putStrLn
        [ relD (W00 999) (W01 0 ""), relD (W00 2) (W00 1), relD (W01 1 "b") (W01 1 "a"), relD W02 W12, relD (W03 (Leaf 1 "b")) (W03 (Leaf 1 "a"))
        , relD (W04 Gamma 0) (W04 Alpha 9), relD (W05 [1, 2]) (W05 [1, 2, 3]), relD (W06 Nothing) (W06 (Just 0)), relD (W07 1 2 3) (W07 1 2 4)
        , relD (W08 (1, "b")) (W08 (1, "a")), relD (W09 (W39 W02)) (W09 (W39 (W00 0))), relD (W39 (W00 0)) (W38 (9, "z")), relD (W17 1 2 3) (W17 1 2 3)
        ]
    mapM_ putStrLn [relHidden (Hidden (W07 1 2 3) (W07 1 2 4)), relHidden (Hidden (W39 W02) (W00 0)), relHidden (Hidden W22 W22)]
    print (length widen)
    print (sort sample)
    print (maximum sample, minimum sample, max (W00 1) (W00 2), max (W00 2) (W00 1), min (W01 1 "b") (W01 1 "a"), min (W39 (W00 0)) W02)
    print (length (filter (> W19 (W00 0)) sample), length (filter (== W00 7) sample), W12 `elem` sample, W13 (Leaf 0 "") `elem` sample)
    print (zipWith compare sample (tail sample))
    print (sort [Leaf 2 "a", Leaf 1 "z", Leaf 1 "a"], sort [Gamma, Alpha, Beta], [minBound .. maxBound :: Tag])
