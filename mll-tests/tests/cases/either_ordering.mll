fromEither :: Either a a -> a
fromEither (Left x) = x
fromEither (Right x) = x

compareInt :: Int -> Int -> Ordering
compareInt a b
    | a < b     = LT
    | a == b    = EQ
    | otherwise = GT

data Prio = Low | Mid | High deriving (Show, Eq, Ord)

main :: IO ()
main = do
    assert (fromEither (Right 42) == 42) "fromEither Right"
    assert (fromEither (Left 99) == 99) "fromEither Left"
    let c = compareInt 1 2
    assert (not (c == EQ)) "compareInt 1 2 is not EQ"
    -- Prelude `compare` (built-in Ord method) on base types
    assert (compare 1 2 == LT) "compare Int LT"
    assert (compare 2 2 == EQ) "compare Int EQ"
    assert (compare 3 2 == GT) "compare Int GT"
    assert (compare "abc" "abd" == LT) "compare String LT"
    assert (compare 2.5 1.5 == GT) "compare Number GT"
    -- Either's own derived Eq/Ord (GHC parity: Left < Right,
    -- lexicographic within a constructor) — Either derived only Show
    -- before round-3 Q48
    assert (Left 1 == (Left 1 :: Either Int Bool)) "Either Eq same"
    assert (not (Left 1 == (Left 2 :: Either Int Bool))) "Either Eq differs"
    assert ((Left 9 :: Either Int Bool) < Right False) "Left < Right"
    assert (compare (Right 2) (Right 1 :: Either Bool Int) == GT) "Right payload compare"
    assert (compare (Left 1) (Left 2 :: Either Int Bool) == LT) "Left payload compare"
    -- compare via derived Ord on a user enum
    assert (compare Low High == LT) "compare enum LT"
    assert (compare High High == EQ) "compare enum EQ"
    assert (compare High Low == GT) "compare enum GT"
    -- Ordering is Showable
    assert (show LT == "LT") "show LT"
    assert (show (compare 5 1) == "GT") "show compare result"
