-- W66 (O10, M15): derived Eq/Ord of a LuaDict string enum. Its values are
-- Lua strings at runtime (the `as` tag or the constructor name), chosen
-- here so that the strings sort AGAINST declaration order; the derived
-- methods read the declaration index through the type's string-to-index
-- table and must follow H2010 §11.1 declaration order like any enum,
-- specialized and through an Ord dictionary (`relD` is used at more
-- types than the specialization limit; `relHidden` through an existential).

data Level = Zero as "zzz" | One as "yyy" | Two as "b" | Three as "a"
    deriving (Eq, Ord, Show, Enum, Bounded, LuaDict)

data Hidden = forall a. (Ord a, Show a) => Hidden a a

relD :: Ord a => a -> a -> [Bool]
relD x y = [compare x y == LT, x == y, x /= y, x < y, x <= y, x > y, x >= y, max x y == y, min x y == x]

relHidden :: Hidden -> [Bool]
relHidden (Hidden x y) = relD x y

widen :: [[Bool]]
widen =
    [ relD (1 :: Int) 2, relD (2.5 :: Number) 1.5, relD "a" "b", relD True False, relD () ()
    , relD LT GT, relD [1 :: Int] [], relD (Just (1 :: Int)) Nothing, relD (1 :: Int, "a") (1, "b")
    , relD ["a"] ["a"], relD (Just "x") (Just "y"), relD (1 :: Int, 2 :: Int, 3 :: Int) (1, 2, 4)
    , relD [[1 :: Int]] [[2]], relD (Just (Just True)) (Just Nothing), relD (True, "t") (False, "t")
    , relD [2.5 :: Number] [1.5], relD (Just (1 :: Int, 2 :: Int)) Nothing, relD [True] [False]
    , relD (Left 1 :: Either Int String) (Right "r")
    ]

main :: IO ()
main = do
    assert (compare Zero Three == LT) "Zero < Three by declaration although \"zzz\" > \"a\""
    assert (compare Three Zero == GT) "Three > Zero by declaration"
    assert (compare One One == EQ) "same constructor is EQ"
    assert (Zero < One && One < Two && Two < Three) "adjacent pairs ascend"
    assert (not (Three < Two) && Three >= Two && Two <= Two && Two >= Two) "the four operators follow the index"
    assert (Zero == Zero && not (Zero == Three) && Zero /= Three && not (One /= One)) "derived Eq compares constructors"
    assert (max Zero Three == Three && max Three Zero == Three && min Zero Three == Zero && min Three Zero == Zero) "max/min select by the index"
    assert (sort [Three, Zero, Two, One] == [Zero, One, Two, Three]) "sort follows declaration order"
    assert (maximum [One, Three, Zero] == Three && minimum [One, Three, Zero] == Zero) "maximum/minimum"
    assert (Two `elem` [Zero, Two] && not (Two `elem` [Zero, Three])) "elem through derived Eq"
    assert (map fromEnum [Zero, One, Two, Three] == [0, 1, 2, 3]) "fromEnum agrees with the order"
    assert ([minBound .. maxBound] == [Zero, One, Two, Three]) "Bounded/Enum range is the declaration order"
    assert (relD Zero Three == [True, False, True, True, True, False, False, True, True]) "dictionary-passing compare on the string enum"
    assert (relD Three Three == [False, True, False, False, True, False, True, True, True]) "dictionary-passing compare on equal values"
    assert (relHidden (Hidden Two One) == [False, False, True, False, False, True, True, False, False]) "compare through a captured dictionary"
    assert (length widen == 19) "widen ran"
    assert (show Three == "Three") "show is the constructor name, not the tag"
    putStrLn "ok"
