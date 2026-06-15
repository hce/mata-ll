-- Tests for derived Eq on types with non-primitive fields
-- Exercises the monomorphizer's polymorphic == resolution

-- Recursive type
data Tree a = Leaf a | Branch (Tree a) (Tree a)
    deriving (Show, Eq)

-- Single constructor with list field
data Bag a = MkBag [a]
    deriving (Show, Eq)

-- Single constructor with Maybe field
data Optional a = MkOptional (Maybe a)
    deriving (Show, Eq)

-- Mixed: primitive and non-primitive fields
data Labeled a = MkLabeled String [a]
    deriving (Show, Eq)

-- Multiple constructors, one with list field
data Collection a = Empty | Items [a]
    deriving (Show, Eq)

-- Nested: list of Maybe
data MaybeList a = MkMaybeList [Maybe a]
    deriving (Show, Eq)

-- Two non-primitive fields
data Pair2 a = MkPair2 [a] [a]
    deriving (Show, Eq)

-- Type with concrete non-primitive field (no type vars)
data Config = MkConfig String [Integer]
    deriving (Show, Eq)

-- Recursive type with extra field
data RoseTree a = RoseLeaf a | RoseNode a [RoseTree a]
    deriving Eq

main :: IO ()
main = do
    -- Recursive tree: same structure
    let t1 = Branch (Leaf 1) (Branch (Leaf 2) (Leaf 3))
    let t2 = Branch (Leaf 1) (Branch (Leaf 2) (Leaf 3))
    assert (t1 == t2) "tree eq same"
    assert (t1 /= Branch (Leaf 1) (Leaf 2)) "tree neq diff structure"
    assert (Leaf 1 /= Leaf 2) "tree neq diff leaf"
    assert (Leaf "hello" == Leaf "hello") "tree eq string leaf"

    -- List field
    assert (MkBag [1, 2, 3] == MkBag [1, 2, 3]) "bag eq"
    assert (MkBag [1, 2, 3] /= MkBag [1, 2, 4]) "bag neq elem"
    assert (MkBag [1, 2] /= MkBag [1, 2, 3]) "bag neq len"
    assert (MkBag ([] :: [Integer]) == MkBag []) "bag eq empty"

    -- Maybe field
    assert (MkOptional (Just 42) == MkOptional (Just 42)) "optional eq just"
    assert (MkOptional (Nothing :: Maybe Integer) == MkOptional Nothing) "optional eq nothing"
    assert (MkOptional (Just 1) /= MkOptional (Just 2)) "optional neq"
    assert (MkOptional (Just 1) /= MkOptional Nothing) "optional neq just/nothing"

    -- Mixed fields
    assert (MkLabeled "x" [1, 2] == MkLabeled "x" [1, 2]) "labeled eq"
    assert (MkLabeled "x" [1, 2] /= MkLabeled "y" [1, 2]) "labeled neq str"
    assert (MkLabeled "x" [1, 2] /= MkLabeled "x" [1, 3]) "labeled neq list"

    -- Multi-constructor with list
    assert ((Empty :: Collection Integer) == Empty) "collection eq empty"
    assert (Items [1, 2] == Items [1, 2]) "collection eq items"
    assert (Items [1, 2] /= Items [1, 3]) "collection neq items"
    assert ((Empty :: Collection Integer) /= Items []) "collection neq empty/items"

    -- Nested: list of Maybe
    assert (MkMaybeList [Just 1, Nothing] == MkMaybeList [Just 1, Nothing]) "maybelist eq"
    assert (MkMaybeList [Just 1] /= MkMaybeList [Just 2]) "maybelist neq"

    -- Two list fields
    assert (MkPair2 [1, 2] [3, 4] == MkPair2 [1, 2] [3, 4]) "pair2 eq"
    assert (MkPair2 [1, 2] [3, 4] /= MkPair2 [1, 2] [3, 5]) "pair2 neq second"
    assert (MkPair2 [1, 2] [3, 4] /= MkPair2 [1, 3] [3, 4]) "pair2 neq first"

    -- Concrete non-primitive field
    assert (MkConfig "dev" [8080, 443] == MkConfig "dev" [8080, 443]) "config eq"
    assert (MkConfig "dev" [8080] /= MkConfig "prod" [8080]) "config neq str"
    assert (MkConfig "dev" [8080] /= MkConfig "dev" [443]) "config neq list"

    -- Rose tree (recursive with list of children)
    let r1 = RoseNode 1 [RoseLeaf 2, RoseLeaf 3]
    let r2 = RoseNode 1 [RoseLeaf 2, RoseLeaf 3]
    let r3 = RoseNode 1 [RoseLeaf 2, RoseLeaf 4]
    assert (r1 == r2) "rose eq"
    assert (r1 /= r3) "rose neq"
    assert (RoseLeaf 5 == RoseLeaf 5) "rose leaf eq"

    -- /= is the negation of ==
    assert (not (t1 /= t2)) "tree not-neq"
    assert (not (MkBag [1] == MkBag [2])) "bag not-eq"

    putStrLn "all derived Eq tests passed"
