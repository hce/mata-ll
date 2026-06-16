-- Feature interaction tests
-- Tests combinations of features that might break when used together

data Tree a = Leaf a | Node (Tree a) (Tree a)
    deriving (Show, Eq)

data Result a = Ok a | Err String
    deriving (Show, Eq)

-- ============================================================
-- Guards + where + pattern matching
-- ============================================================

classifyTree :: Tree Integer -> String
classifyTree (Leaf n)
    | n < 0     = "negative leaf: " <> label
    | n == 0    = "zero leaf"
    | otherwise = "positive leaf: " <> label
    where label = show n
classifyTree (Node l r)
    | dl > dr   = "left-heavy"
    | dl < dr   = "right-heavy"
    | otherwise = "balanced"
    where dl = treeDepth l
          dr = treeDepth r

treeDepth :: Tree a -> Integer
treeDepth (Leaf _) = 0
treeDepth (Node l r) = 1 + max (treeDepth l) (treeDepth r)

-- ============================================================
-- List comprehension + pattern matching + guards
-- ============================================================

-- Explicit recursion style (pattern-matching generators also work now)
getOks :: [Result Integer] -> [Integer]
getOks [] = []
getOks (Ok x : rest) = x : getOks rest
getOks (Err _ : rest) = getOks rest

-- ============================================================
-- Higher-order + typeclasses
-- ============================================================

applyAndShow :: Show b => (a -> b) -> a -> String
applyAndShow f x = show (f x)

-- ============================================================
-- Case in guards
-- ============================================================

safeDiv :: Integer -> Integer -> Result Integer
safeDiv _ 0 = Err "division by zero"
safeDiv a b = Ok (a `div` b)

describeSafe :: Integer -> Integer -> String
describeSafe a b
    | isOk result = "ok: " <> show (getVal result)
    | otherwise   = "error"
    where result = safeDiv a b

isOk :: Result a -> Bool
isOk (Ok _) = True
isOk (Err _) = False

getVal :: Result a -> a
getVal (Ok x) = x
getVal (Err _) = error "getVal on Err"

-- ============================================================
-- Recursion + accumulator + higher-order
-- ============================================================

myMap :: (a -> b) -> [a] -> [b]
myMap _ [] = []
myMap f (x:xs) = f x : myMap f xs

myFilter :: (a -> Bool) -> [a] -> [a]
myFilter _ [] = []
myFilter p (x:xs)
    | p x = x : myFilter p xs
    | otherwise = myFilter p xs

myZipWith :: (a -> b -> c) -> [a] -> [b] -> [c]
myZipWith _ [] _ = []
myZipWith _ _ [] = []
myZipWith f (a:as) (b:bs) = f a b : myZipWith f as bs

-- ============================================================
-- Where clause with multiple bindings referencing each other
-- ============================================================

quadratic :: Number -> Number -> Number -> Number -> Number
quadratic a b c x = a * xsq + b * x + c
    where xsq = x * x

-- ============================================================
-- Data types in data types
-- ============================================================

data Pair a b = MkPair a b
    deriving (Show, Eq)

swap :: Pair a b -> Pair b a
swap (MkPair a b) = MkPair b a

-- ============================================================
-- Newtype + operations
-- ============================================================

newtype Celsius = Celsius Number
newtype Fahrenheit = Fahrenheit Number

toFahrenheit :: Celsius -> Fahrenheit
toFahrenheit (Celsius c) = Fahrenheit (c * 1.8 + 32.0)

fromFahrenheit :: Fahrenheit -> Celsius
fromFahrenheit (Fahrenheit f) = Celsius ((f - 32.0) / 1.8)

main :: IO ()
main = do
    -- Guards + where + pattern matching
    assert (classifyTree (Leaf (-5)) == "negative leaf: -5") "guard+where+pat neg"
    assert (classifyTree (Leaf 0) == "zero leaf") "guard+where+pat zero"
    assert (classifyTree (Leaf 3) == "positive leaf: 3") "guard+where+pat pos"
    assert (classifyTree (Node (Leaf 1) (Node (Leaf 2) (Leaf 3))) == "right-heavy") "tree classify"
    assert (classifyTree (Node (Leaf 1) (Leaf 2)) == "balanced") "tree balanced"

    -- Filter by constructor
    let results = [Ok 1, Err "bad", Ok 2, Err "nope", Ok 3]
    assert (getOks results == [1, 2, 3]) "filter constructors"
    assert (getOks [] == ([] :: [Integer])) "filter empty"

    -- Higher-order + show
    assert (applyAndShow (* 2) 5 == "10") "apply and show"

    -- Case in guards via where
    assert (describeSafe 10 3 == "ok: 3") "safe div ok"
    assert (describeSafe 10 0 == "error") "safe div err"

    -- Custom higher-order
    assert (myMap (* 2) [1, 2, 3] == [2, 4, 6]) "myMap"
    assert (myFilter (> 2) [1, 2, 3, 4] == [3, 4]) "myFilter"
    assert (myZipWith (+) [1, 2, 3] [10, 20, 30] == [11, 22, 33]) "myZipWith"
    assert (myZipWith (+) [1, 2] [10, 20, 30] == [11, 22]) "myZipWith short"

    -- Where with dependencies
    assert (quadratic 1.0 0.0 0.0 3.0 == 9.0) "quadratic x^2"
    assert (quadratic 1.0 2.0 1.0 3.0 == 16.0) "quadratic full"

    -- Nested data types
    assert (swap (MkPair 1 "hello") == MkPair "hello" 1) "swap pair"
    assert (MkPair 1 2 == MkPair 1 2) "pair eq"
    assert (MkPair 1 2 /= MkPair 1 3) "pair neq"

    -- Newtype operations
    let c = Celsius 100.0
    let f = case toFahrenheit c of { Fahrenheit x -> x }
    assert (f == 212.0) "celsius to fahrenheit"

    -- Chain of operations
    let nums = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
    let result = foldl (+) 0 (myFilter (> 5) (myMap (* 2) nums))
    assert (result == 6 + 8 + 10 + 12 + 14 + 16 + 18 + 20) "chain map filter fold"

    -- List comprehension + function application
    let squares = [x * x | x <- [1, 2, 3, 4, 5]]
    assert (squares == [1, 4, 9, 16, 25]) "squares comp"
    assert (foldl (+) 0 squares == 55) "sum of squares"
