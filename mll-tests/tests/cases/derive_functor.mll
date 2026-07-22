-- Test deriving Functor

-- Simple wrapper
data Box a = MkBox a
    deriving (Show, Eq, Functor)

-- Constructor with no fields mentioning the type param
data Tagged a = Tag String a
    deriving (Show, Eq, Functor)

-- Multiple fields of the functor parameter
data Triple a = MkTriple a a a
    deriving (Show, Eq, Functor)

-- Phantom type parameter (no constructor mentions it)
data Phantom a = MkPhantom Integer
    deriving (Show, Eq, Functor)

-- Multi-param type (Functor on last param only)
data Pair a b = MkPair a b
    deriving (Show, Eq, Functor)

-- Multiple constructors (non-recursive)
data Result a = Ok a | Err String
    deriving (Show, Eq, Functor)

-- Binary tree (recursive, multiple constructors)
data Tree a = Leaf a | Branch (Tree a) (Tree a)
    deriving (Show, Eq, Functor)

-- Nested functor: field contains [a]
data WithList a = MkWithList [a]
    deriving (Eq, Functor)

-- Nested functor: field contains Maybe a
data WithMaybe a = MkWithMaybe (Maybe a)
    deriving (Eq, Functor)

main :: IO ()
main = do
    -- Simple wrapper
    assert (fmap (+1) (MkBox 5) == MkBox 6) "fmap Box"
    assert (fmap show (MkBox 42) == MkBox "42") "fmap Box show"

    -- Tagged (mixed fields)
    assert (fmap (*2) (Tag "hello" 5) == Tag "hello" 10) "fmap Tagged"

    -- Triple (multiple same-type fields)
    assert (fmap (+10) (MkTriple 1 2 3) == MkTriple 11 12 13) "fmap Triple"

    -- Phantom (no fields to map)
    assert (fmap (+1) (MkPhantom 42) == MkPhantom 42) "fmap Phantom"

    -- Multi-param (maps over last param only)
    assert (fmap (+1) (MkPair "hello" 5) == MkPair "hello" 6) "fmap Pair"

    -- Multiple constructors
    assert (fmap (+1) (Ok 5) == Ok 6) "fmap Result Ok"
    assert (fmap (+1) (Err "oops") == Err "oops") "fmap Result Err"

    -- <$> operator alias
    assert (((+1) <$> MkBox 10) == MkBox 11) "<$> Box"

    -- Binary tree (recursive, derived Eq works with the fix)
    let tree = Branch (Leaf 1) (Branch (Leaf 2) (Leaf 3))
    let result = fmap (+1) tree
    assert (result == Branch (Leaf 2) (Branch (Leaf 3) (Leaf 4))) "fmap Tree"
    assert (show (fmap (*10) (Leaf 7)) == "Leaf 70") "fmap Tree Leaf"

    -- Nested: list field (derived Eq calls eq_[] instead of Lua ==)
    assert (fmap (+1) (MkWithList [1, 2, 3]) == MkWithList [2, 3, 4]) "fmap WithList"

    -- Nested: Maybe field (derived Eq calls eq_Maybe instead of Lua ==)
    assert (fmap (+1) (MkWithMaybe (Just 5)) == MkWithMaybe (Just 6)) "fmap WithMaybe Just"
    assert (fmap (+1) (MkWithMaybe Nothing) == MkWithMaybe Nothing) "fmap WithMaybe Nothing"

    putStrLn "all deriving Functor tests passed"
