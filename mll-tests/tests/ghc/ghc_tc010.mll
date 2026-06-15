-- GHC tc010: Functor on custom type
-- Derive Functor, verify functor laws hold

data Box a = Empty | Full a
    deriving (Show, Eq, Functor)

data Pair a = Pair a a
    deriving (Show, Eq, Functor)

data Rose a = RLeaf a | RNode [Rose a]
    deriving (Show, Eq)

fmapRose :: (a -> b) -> Rose a -> Rose b
fmapRose f (RLeaf x)  = RLeaf (f x)
fmapRose f (RNode cs) = RNode (map (fmapRose f) cs)

double :: Integer -> Integer
double x = x * 2

addTen :: Integer -> Integer
addTen x = x + 10

-- Functor law 1: fmap id == id
-- Functor law 2: fmap (f . g) == fmap f . fmap g

main :: IO ()
main = do
    -- Box functor
    assert (fmap double Empty == Empty) "fmap empty"
    assert (fmap double (Full 5) == Full 10) "fmap full"
    assert (fmap addTen (Full 3) == Full 13) "fmap addTen"

    -- Law 1: fmap id = id
    assert (fmap id (Full 7) == Full 7) "law1 full"
    assert (fmap id Empty == (Empty :: Box Integer)) "law1 empty"

    -- Law 2: composition
    assert (fmap (addTen . double) (Full 3) == fmap addTen (fmap double (Full 3))) "law2 full"
    assert (fmap (addTen . double) Empty == fmap addTen (fmap double (Empty :: Box Integer))) "law2 empty"

    -- Pair functor
    assert (fmap double (Pair 3 4) == Pair 6 8) "pair fmap"
    assert (fmap id (Pair 1 2) == Pair 1 2) "pair law1"

    -- Rose tree manual fmap
    let tree = RNode [RLeaf 1, RNode [RLeaf 2, RLeaf 3], RLeaf 4]
    let doubled = fmapRose double tree
    assert (doubled == RNode [RLeaf 2, RNode [RLeaf 4, RLeaf 6], RLeaf 8]) "rose fmap"
    assert (fmapRose id (RLeaf 5) == RLeaf 5) "rose law1"

    putStrLn "ok"
