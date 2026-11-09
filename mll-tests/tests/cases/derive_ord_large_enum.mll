-- W66 (O10, M15): a derived `Ord`/`Eq` of a 100-constructor enumeration.
-- H2010 §11.1 derived Ord orders constructors by declaration position;
-- the derived methods used to be one clause per constructor PAIR, a
-- quadratic text LuaJIT refused to load from 66 constructors ("control
-- structure too long"), and are now a constant-size index comparison.
-- Pinned against GHC: compare and the six operators on first/last/
-- adjacent/equal pairs, max/min and their tie rule, sort, minimum and
-- maximum, elem and lookup, Enum/Bounded round trips, and the values
-- inside Data.Map (a user type is not a key type here — HASKDIFF — so
-- they are the values: compared by the map's Ord, filtered, sorted). `rel` stays specialized;
-- `relD` is used at more types than the specialization limit (`widen`)
-- and runs every comparison through an Ord dictionary, and `relHidden`
-- hands it the dictionary an existential captured.
module Main where

import qualified Data.Map as M

data Big
    = C00 | C01 | C02 | C03 | C04 | C05 | C06 | C07 | C08 | C09
    | C10 | C11 | C12 | C13 | C14 | C15 | C16 | C17 | C18 | C19
    | C20 | C21 | C22 | C23 | C24 | C25 | C26 | C27 | C28 | C29
    | C30 | C31 | C32 | C33 | C34 | C35 | C36 | C37 | C38 | C39
    | C40 | C41 | C42 | C43 | C44 | C45 | C46 | C47 | C48 | C49
    | C50 | C51 | C52 | C53 | C54 | C55 | C56 | C57 | C58 | C59
    | C60 | C61 | C62 | C63 | C64 | C65 | C66 | C67 | C68 | C69
    | C70 | C71 | C72 | C73 | C74 | C75 | C76 | C77 | C78 | C79
    | C80 | C81 | C82 | C83 | C84 | C85 | C86 | C87 | C88 | C89
    | C90 | C91 | C92 | C93 | C94 | C95 | C96 | C97 | C98 | C99
    deriving (Eq, Ord, Show, Enum, Bounded)

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

every :: [Big]
every = [minBound .. maxBound]

main :: IO ()
main = do
    mapM_ putStrLn [rel C00 C99, rel C99 C00, rel C42 C43, rel C43 C42, rel C57 C57, rel C00 C00, rel C99 C99, rel C65 C66]
    mapM_ putStrLn [relD C00 C99, relD C99 C00, relD C42 C43, relD C57 C57, relD C98 C99]
    mapM_ putStrLn [relHidden (Hidden C07 C70), relHidden (Hidden C70 C07), relHidden (Hidden C33 C33)]
    print (length widen)
    print (max C10 C90, max C90 C10, min C10 C90, min C90 C10, max C45 C45, min C45 C45)
    print (maximum [C03, C60, C07, C59], minimum [C03, C60, C07, C59], maximum every, minimum every)
    print (sort [C99, C05, C50, C00, C72, C05, C13], sort (reverse every) == every)
    print (C66 `elem` every, lookup C77 (zip every [0 :: Int ..]), length (filter (> C89) every), length (filter (<= C09) every))
    print (map fromEnum [C00, C01, C65, C66, C99], map (toEnum :: Int -> Big) [0, 65, 99], succ C65, pred C66, [C97 ..])
    print (length every, take 3 every, [C00, C25 ..], length (filter (== C50) every), zipWith compare every (tail every) == replicate 99 LT)
    let m = M.fromList [(show k, k) | k <- [C90, C05, C50, C99, C00]]
        m2 = M.insert "C50" C51 m
    print (M.toList m, M.elems m == sort (M.elems m), M.lookup "C50" m, maximum (M.elems m), minimum (M.elems m))
    print (compare m m2, m == m2, m < m2, max m m2 == m2, M.toList (M.filter (> C49) m), M.toList (M.map succ (M.delete "C99" m)))
    print (M.toList (M.fromList (zip (map show every) every)) == zip (map show every) (sort every), M.lookup "C66" (M.fromList (zip (map show every) every)))
