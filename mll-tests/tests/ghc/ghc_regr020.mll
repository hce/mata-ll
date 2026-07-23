-- ghc_regr020: /= operator on all type shapes (regression for /= fix)

data Color = Red | Green | Blue
    deriving (Show, Eq)

data Shape = Circle Number | Rect Number Number
    deriving (Show, Eq)

data Tree a = Leaf | Node (Tree a) a (Tree a)
    deriving (Show, Eq)

data Pair a b = Pair a b
    deriving (Show, Eq)

main :: IO ()
main = do
    -- Int /=
    assert (1 /= (2 :: Int)) "int neq"
    assert (not (1 /= (1 :: Int))) "int eq via /="
    assert (0 /= (-1 :: Int)) "zero neq neg"

    -- String /=
    assert ("abc" /= "abd") "str neq"
    assert (not ("abc" /= "abc")) "str eq via /="
    assert ("" /= " ") "empty neq space"

    -- Bool /=
    assert (True /= False) "bool neq"
    assert (not (True /= True)) "bool eq via /="

    -- Custom enum /=
    assert (Red /= Blue) "color neq"
    assert (not (Red /= Red)) "color eq via /="

    -- Algebraic with fields /=
    assert (Circle 1.0 /= Circle 2.0) "circle neq"
    assert (not (Circle 1.0 /= Circle 1.0)) "circle eq via /="
    assert (Circle 1.0 /= Rect 1.0 1.0) "circle neq rect"

    -- Maybe /=
    assert (Just (1 :: Int) /= Just 2) "maybe just neq"
    assert (not (Just (1 :: Int) /= Just 1)) "maybe eq via /="
    assert ((Nothing :: Maybe Int) /= Just 1) "nothing neq just"

    -- List /=
    assert ([1, 2, 3 :: Int] /= [1, 2, 4]) "list neq last"
    assert ([1 :: Int] /= [1, 2]) "list neq length"
    assert (not ([1, 2 :: Int] /= [1, 2])) "list eq via /="

    -- Tuple /=
    assert ((1 :: Int, 2 :: Int) /= (1, 3)) "tuple neq snd"
    assert (not ((1 :: Int, 2 :: Int) /= (1, 2))) "tuple eq via /="

    -- Recursive Tree /=
    let t1 = Node Leaf (1 :: Int) Leaf
    let t2 = Node Leaf (2 :: Int) Leaf
    assert (t1 /= t2) "tree neq"
    assert (not (t1 /= t1)) "tree eq via /="
    assert ((Leaf :: Tree Int) /= t1) "leaf neq node"

    -- Polymorphic Pair /=
    assert (Pair (1 :: Int) True /= Pair 1 False) "pair neq"
    assert (not (Pair (1 :: Int) True /= Pair 1 True)) "pair eq via /="

    putStrLn "ok"
