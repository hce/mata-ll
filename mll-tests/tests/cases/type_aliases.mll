-- Comprehensive type alias tests

type Pair a = (a, a)
type IntPair = Pair Int
type StringList = [String]
type Predicate a = a -> Bool
type Transform a = a -> a

-- Using type aliases in signatures
swap :: Pair a -> Pair a
swap (x, y) = (y, x)

both :: Predicate a -> Pair a -> Bool
both p (x, y) = p x && p y

applyBoth :: Transform a -> Pair a -> Pair a
applyBoth f (x, y) = (f x, f y)

-- Int as alias for Int
addInts :: Int -> Int -> Int
addInts x y = x + y

-- Nested aliases
type MaybeInt = Maybe Int
type MaybeList a = Maybe [a]

fromMaybeInt :: MaybeInt -> Int
fromMaybeInt (Just x) = x
fromMaybeInt Nothing = 0

main :: IO ()
main = do
    -- Basic pair alias
    let p = (1, 2) :: IntPair
    assert (fst p == 1) "int pair fst"
    assert (snd p == 2) "int pair snd"

    -- Swap
    assert (fst (swap (1, 2)) == 2) "swap fst"
    assert (snd (swap (1, 2)) == 1) "swap snd"

    -- Predicate alias
    let isPositive = \x -> x > 0
    assert (both isPositive (1, 2) == True) "both positive"
    assert (both isPositive (1, -1) == False) "both not positive"

    -- Transform alias
    assert (fst (applyBoth (* 2) (3, 4)) == 6) "transform fst"
    assert (snd (applyBoth (* 2) (3, 4)) == 8) "transform snd"

    -- Int alias
    assert (addInts 3 4 == 7) "int alias"

    -- Nested alias
    assert (fromMaybeInt (Just 42) == 42) "maybe int just"
    assert (fromMaybeInt Nothing == 0) "maybe int nothing"

    -- StringList alias
    let names = ["alice", "bob"] :: StringList
    assert (head names == "alice") "string list"
    assert (length names == 2) "string list len"
