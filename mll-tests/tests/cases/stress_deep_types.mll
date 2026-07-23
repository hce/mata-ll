-- Stress test: deeply nested data types and recursive structures

data Tree = Leaf Int | Branch Tree Tree
    deriving (Show, Eq)

completeBTree :: Int -> Int -> Tree
completeBTree val 0 = Leaf val
completeBTree val d = Branch (completeBTree (val * 2) (d - 1)) (completeBTree (val * 2 + 1) (d - 1))

countNodes :: Tree -> Int
countNodes (Leaf _) = 1
countNodes (Branch l r) = 1 + countNodes l + countNodes r

sumLeaves :: Tree -> Int
sumLeaves (Leaf n) = n
sumLeaves (Branch l r) = sumLeaves l + sumLeaves r

treeDepth :: Tree -> Int
treeDepth (Leaf _) = 0
treeDepth (Branch l r) = 1 + maxInt (treeDepth l) (treeDepth r)

maxInt :: Int -> Int -> Int
maxInt a b = if a > b then a else b

mirror :: Tree -> Tree
mirror (Leaf n) = Leaf n
mirror (Branch l r) = Branch (mirror r) (mirror l)

data NestM = NestM (Maybe (Maybe (Maybe Int)))
    deriving (Show, Eq)

unwrapNest :: NestM -> Int
unwrapNest (NestM (Just (Just (Just n)))) = n
unwrapNest _ = -1

data Result a = Ok a | Err String
    deriving (Show, Eq)

data Nested = N1 (Result (Result (Result Int)))
    deriving (Show, Eq)

deepUnwrap :: Nested -> Int
deepUnwrap (N1 (Ok (Ok (Ok n)))) = n
deepUnwrap _ = -1

treeList :: Int -> [Tree]
treeList 0 = []
treeList n = completeBTree 1 3 : treeList (n - 1)

main :: IO ()
main = do
    let t5 = completeBTree 1 5
    assert (treeDepth t5 == 5) "depth 5"
    assert (countNodes t5 == 63) "nodes in depth-5 tree"
    let t8 = completeBTree 1 8
    assert (treeDepth t8 == 8) "depth 8"
    assert (countNodes t8 == 511) "nodes in depth-8 tree"
    assert (treeDepth (mirror t5) == 5) "mirror preserves depth"
    assert (countNodes (mirror t5) == 63) "mirror preserves count"
    assert (mirror (mirror t5) == t5) "double mirror is identity"
    let nm = NestM (Just (Just (Just 42)))
    assert (unwrapNest nm == 42) "unwrap nested maybe"
    assert (unwrapNest (NestM Nothing) == -1) "unwrap nothing"
    let dn = N1 (Ok (Ok (Ok 99)))
    assert (deepUnwrap dn == 99) "deep unwrap ok"
    assert (deepUnwrap (N1 (Err "bad")) == -1) "deep unwrap err"
    let ts = treeList 20
    assert (length ts == 20) "20 trees"
    putStrLn "ok"
