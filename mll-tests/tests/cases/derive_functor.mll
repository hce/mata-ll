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
    deriving Functor

-- Helper: manually show a Tree Integer
showTree :: Tree Integer -> String
showTree (Leaf n) = "Leaf " ++ show n
showTree (Branch l r) = "Branch (" ++ showTree l ++ ") (" ++ showTree r ++ ")"

-- Helper: manually compare Tree Integer
eqTree :: Tree Integer -> Tree Integer -> Bool
eqTree (Leaf a) (Leaf b) = a == b
eqTree (Branch l1 r1) (Branch l2 r2) = eqTree l1 l2 && eqTree r1 r2
eqTree _ _ = False

-- Nested functor: field contains [a]
data WithList a = MkWithList [a]
    deriving Functor

getList :: WithList a -> [a]
getList (MkWithList xs) = xs

-- Nested functor: field contains Maybe a
data WithMaybe a = MkWithMaybe (Maybe a)
    deriving Functor

getMaybe :: WithMaybe a -> Maybe a
getMaybe (MkWithMaybe m) = m

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
    assert ((+1) <$> MkBox 10 == MkBox 11) "<$> Box"

    -- Binary tree (recursive, use manual helpers)
    let tree = Branch (Leaf 1) (Branch (Leaf 2) (Leaf 3))
    let result = fmap (+1) tree
    assert (eqTree result (Branch (Leaf 2) (Branch (Leaf 3) (Leaf 4)))) "fmap Tree"
    assert (showTree (fmap (*10) (Leaf 7)) == "Leaf 70") "fmap Tree Leaf"

    -- Nested: list field (extract and compare the list directly)
    assert (getList (fmap (+1) (MkWithList [1, 2, 3])) == [2, 3, 4]) "fmap WithList"

    -- Nested: Maybe field
    assert (getMaybe (fmap (+1) (MkWithMaybe (Just 5))) == Just 6) "fmap WithMaybe Just"
    assert (getMaybe (fmap (+1) (MkWithMaybe Nothing)) == Nothing) "fmap WithMaybe Nothing"

    putStrLn "all deriving Functor tests passed"
