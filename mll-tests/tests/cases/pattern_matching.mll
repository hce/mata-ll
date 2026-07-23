-- Comprehensive pattern matching tests

data Tree a = Leaf a | Branch (Tree a) (Tree a)
    deriving (Show, Eq)

data Expr = Num Int | Add Expr Expr | Mul Expr Expr

-- Basic constructor patterns
eval :: Expr -> Int
eval (Num n) = n
eval (Add a b) = eval a + eval b
eval (Mul a b) = eval a * eval b

-- Nested patterns
depth :: Tree a -> Int
depth (Leaf _) = 0
depth (Branch l r) = 1 + max (depth l) (depth r)

-- Wildcard and variable patterns
first :: (a, b, c) -> a
first (x, _, _) = x

-- Literal patterns
describe :: Int -> String
describe 0 = "zero"
describe 1 = "one"
describe _ = "other"

-- Guard patterns
clamp :: Int -> Int -> Int -> Int
clamp lo hi x
    | x < lo = lo
    | x > hi = hi
    | otherwise = x

-- Guards with where
bmi :: Number -> String
bmi weight
    | weight < thin = "underweight"
    | weight < normal = "normal"
    | otherwise = "overweight"
    where thin = 18.5
          normal = 25.0

-- Nested constructor + literal
isLeafOne :: Tree Int -> Bool
isLeafOne (Leaf 1) = True
isLeafOne _ = False

-- As-patterns (not supported, but wildcard equivalent)
-- Lambda patterns
mapMaybe :: (a -> b) -> Maybe a -> Maybe b
mapMaybe f m = case m of
    Nothing -> Nothing
    Just x -> Just (f x)

main :: IO ()
main = do
    -- Constructor patterns
    assert (eval (Add (Num 3) (Mul (Num 2) (Num 4))) == 11) "eval nested"
    assert (eval (Num 42) == 42) "eval literal"

    -- Nested tree patterns
    let t = Branch (Branch (Leaf 1) (Leaf 2)) (Leaf 3)
    assert (depth t == 2) "tree depth"
    assert (depth (Leaf 0) == 0) "leaf depth"

    -- Tuple patterns
    assert (first (1, 2, 3) == 1) "tuple first"

    -- Literal patterns
    assert (describe 0 == "zero") "literal 0"
    assert (describe 1 == "one") "literal 1"
    assert (describe 99 == "other") "literal wildcard"

    -- Guards
    assert (clamp 0 10 5 == 5) "clamp middle"
    assert (clamp 0 10 (-3) == 0) "clamp low"
    assert (clamp 0 10 15 == 10) "clamp high"

    -- Guards with where
    assert (bmi 15.0 == "underweight") "bmi under"
    assert (bmi 22.0 == "normal") "bmi normal"
    assert (bmi 30.0 == "overweight") "bmi over"

    -- Nested constructor + literal
    assert (isLeafOne (Leaf 1) == True) "leaf one yes"
    assert (isLeafOne (Leaf 2) == False) "leaf one no"
    assert (isLeafOne (Branch (Leaf 1) (Leaf 1)) == False) "leaf one branch"

    -- Lambda patterns
    let inc = \(Just x) -> Just (x + 1)
    assert (inc (Just 5) == Just 6) "lambda pattern Just"

    -- Case with multiple constructors
    assert (mapMaybe (* 2) (Just 5) == Just 10) "mapMaybe Just"
    assert (mapMaybe (* 2) Nothing == Nothing) "mapMaybe Nothing"

    -- Inline case
    let r = case Just 42 of { Just x -> x; Nothing -> 0 }
    assert (r == 42) "inline case"
