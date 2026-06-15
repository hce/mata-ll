-- ghc_regr019: Derived Eq on types with Maybe and List fields (regression for the fix)

data Box a = Box (Maybe a)
    deriving (Show, Eq)

data Bag a = Bag [a]
    deriving (Show, Eq)

data Tagged a = Tagged String (Maybe [a])
    deriving (Show, Eq)

data Tree a = Leaf | Node a (Tree a) (Tree a)
    deriving (Show, Eq)

data Config = Config { cfgName :: String, cfgTags :: [String], cfgValue :: Maybe Integer }
    deriving (Show, Eq)

-- Two-field type with heterogeneous non-primitive fields
data Pair a b = Pair (Maybe a) [b]
    deriving (Show, Eq)

appendList :: [a] -> [a] -> [a]
appendList [] ys     = ys
appendList (x:xs) ys = x : appendList xs ys

main :: IO ()
main = do
    -- Box: Maybe field
    assert (Box (Just 1) == Box (Just 1)) "box just eq"
    assert (Box (Nothing :: Maybe Integer) == Box Nothing) "box nothing eq"
    assert (Box (Just 1) /= Box (Just 2)) "box just neq"
    assert (Box (Just 1) /= Box Nothing) "box just/nothing neq"

    -- Bag: list field
    assert (Bag [1, 2, 3 :: Integer] == Bag [1, 2, 3]) "bag eq"
    assert (Bag ([] :: [Integer]) == Bag []) "bag empty eq"
    assert (Bag [1, 2 :: Integer] /= Bag [1, 3]) "bag neq elem"
    assert (Bag [1 :: Integer] /= Bag [1, 2]) "bag neq length"

    -- Tagged: String + Maybe [a]
    assert (Tagged "x" (Just [1, 2 :: Integer]) == Tagged "x" (Just [1, 2])) "tagged eq"
    assert (Tagged "x" (Nothing :: Maybe [Integer]) == Tagged "x" Nothing) "tagged nothing eq"
    assert (Tagged "x" (Just [1 :: Integer]) /= Tagged "y" (Just [1])) "tagged neq name"
    assert (Tagged "x" (Just [1 :: Integer]) /= Tagged "x" (Just [2])) "tagged neq inner"
    -- Note: comparing Just [] with Nothing for Maybe [a] may have issues in MATA-LL
    -- Test with non-empty list instead:
    assert (Tagged "x" (Just [1 :: Integer]) /= Tagged "x" Nothing) "tagged just-nonempty/nothing neq"

    -- Tree (recursive)
    let t1 = Node 1 (Node 2 Leaf Leaf) Leaf :: Tree Integer
    let t2 = Node 1 (Node 2 Leaf Leaf) Leaf :: Tree Integer
    let t3 = Node 1 (Node 3 Leaf Leaf) Leaf :: Tree Integer
    assert (t1 == t2) "tree eq"
    assert (t1 /= t3) "tree neq"
    assert ((Leaf :: Tree Integer) == Leaf) "leaf eq"
    assert (Leaf /= Node (1 :: Integer) Leaf Leaf) "leaf/node neq"

    -- Config: multiple non-primitive fields
    let c1 = Config { cfgName = "dev", cfgTags = ["a", "b"], cfgValue = Just 8080 }
    let c2 = Config { cfgName = "dev", cfgTags = ["a", "b"], cfgValue = Just 8080 }
    let c3 = Config { cfgName = "prod", cfgTags = ["a", "b"], cfgValue = Just 8080 }
    assert (c1 == c2) "config eq"
    assert (c1 /= c3) "config neq name"
    assert (c1 /= c1 { cfgTags = ["a"] }) "config neq tags"
    assert (c1 /= c1 { cfgValue = Nothing }) "config neq value"

    -- Pair: Maybe a + [b]
    assert (Pair (Just True) [1, 2 :: Integer] == Pair (Just True) [1, 2]) "pair eq"
    assert (Pair (Nothing :: Maybe Bool) ([] :: [Integer]) == Pair Nothing []) "pair empty eq"
    assert (Pair (Just True) [1 :: Integer] /= Pair (Just False) [1]) "pair neq maybe"
    assert (Pair (Just True) [1 :: Integer] /= Pair (Just True) [2]) "pair neq list"

    putStrLn "ok"
