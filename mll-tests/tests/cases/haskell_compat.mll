-- Haskell compatibility test suite
-- Tests features from the Haskell 2010 Report that mata-ll supports.
-- Each section corresponds to a Report chapter.

-- ============================================================
-- Chapter 3: Expressions
-- ============================================================

-- 3.1 Conditionals
test_if :: IO ()
test_if = do
    assert ((if True then 1 else 2) == 1) "if true"
    assert ((if False then 1 else 2) == 2) "if false"
    -- Nested if
    let x = if True then if False then 1 else 2 else 3
    assert (x == 2) "nested if"

-- 3.2 Let expressions
test_let :: IO ()
test_let = do
    let x = let a = 1
                b = 2
            in a + b
    assert (x == 3) "let expr"
    -- Let-in expression
    let y = let a = 10 in a + 5
    assert (y == 15) "let-in expr"

-- 3.3 Lambda abstractions
test_lambda :: IO ()
test_lambda = do
    let f = \x -> x + 1
    assert (f 5 == 6) "lambda"
    let g = \x y -> x * y
    assert (g 3 4 == 12) "multi-param lambda"
    -- Lambda in higher-order
    assert (const 42 99 == 42) "const"

-- 3.4 Case expressions
caseTest :: Integer -> String
caseTest x = case x of
    1 -> "one"
    2 -> "two"
    _ -> "other"

test_case :: IO ()
test_case = do
    assert (caseTest 1 == "one") "case 1"
    assert (caseTest 2 == "two") "case 2"
    assert (caseTest 99 == "other") "case wildcard"

-- 3.5 Operators and sections
test_operators :: IO ()
test_operators = do
    assert ((+) 3 4 == 7) "op as function"
    let double = (* 2)
    assert (double 5 == 10) "left section"
    assert (10 `div` 2 == 5) "backtick infix"

-- ============================================================
-- Chapter 4: Declarations and Bindings
-- ============================================================

-- 4.1 Type signatures
id' :: a -> a
id' x = x

-- 4.2 Function bindings with pattern matching
fib :: Integer -> Integer
fib 0 = 0
fib 1 = 1
fib n = fib (n - 1) + fib (n - 2)

-- 4.3 Guards
classify :: Integer -> String
classify n
    | n < 0     = "negative"
    | n == 0    = "zero"
    | otherwise = "positive"

-- 4.4 Where clauses
circleArea :: Number -> Number
circleArea r = pi' * r * r
    where pi' = 3.14159

-- ============================================================
-- Chapter 5: Data Types
-- ============================================================

-- 5.1 Algebraic data types
data Color = Red | Green | Blue
    deriving (Show, Eq)

data Shape = Circle Number | Rect Number Number

-- 5.2 Records
data Point = Point { pointX :: Number, pointY :: Number }

-- 5.3 Newtypes
newtype Meters = Meters Number

-- 5.4 Maybe and Either (prelude types)
safeDivide :: Integer -> Integer -> Maybe Integer
safeDivide _ 0 = Nothing
safeDivide a b = Just (a `div` b)

-- ============================================================
-- Chapter 6: Typeclasses
-- ============================================================

-- 6.1 Class declarations and instances
data Weekday = Mon | Tue | Wed | Thu | Fri | Sat | Sun
    deriving (Show, Eq)

isWeekend :: Weekday -> Bool
isWeekend Sat = True
isWeekend Sun = True
isWeekend _   = False

-- 6.2 Superclasses (Eq => Ord)
data Priority = Low | Medium | High
    deriving (Show, Eq, Ord)

-- ============================================================
-- Chapter 7: Lists
-- ============================================================

test_lists :: IO ()
test_lists = do
    -- Construction
    assert (head [1, 2, 3] == 1) "head"
    assert (tail [1, 2, 3] == [2, 3]) "tail"
    assert (1 : [2, 3] == [1, 2, 3]) "cons"
    -- Higher-order
    assert (map (* 2) [1, 2, 3] == [2, 4, 6]) "map"
    assert (filter (> 2) [1, 2, 3, 4] == [3, 4]) "filter"
    assert (foldl (+) 0 [1, 2, 3] == 6) "foldl"
    assert (length [1, 2, 3] == 3) "length"
    assert (reverse [1, 2, 3] == [3, 2, 1]) "reverse"

-- ============================================================
-- Chapter 8: IO
-- ============================================================

test_io :: IO ()
test_io = do
    -- Sequencing
    let x = 42
    assert (x == 42) "do let"
    -- Bind
    y <- pure 99
    assert (y == 99) "do bind"
    -- Then
    pure ()
    assert True "do then"

-- ============================================================
-- Chapter 9: Non-strict semantics
-- ============================================================

test_laziness :: IO ()
test_laziness = do
    -- Unused bottom doesn't crash
    let x = undefined
    assert (const 1 x == 1) "unused bottom"
    -- Partial list (only access what's available)
    assert (head [1, 2, 3] == 1) "list head"
    -- Lazy cons
    let xs = 1 : 2 : undefined
    assert (head xs == 1) "lazy cons head"

-- ============================================================
-- Additional: Tuples
-- ============================================================

test_tuples :: IO ()
test_tuples = do
    let p = (1, 2)
    assert (fst p == 1) "fst"
    assert (snd p == 2) "snd"

-- ============================================================
-- Additional: String operations
-- ============================================================

test_strings :: IO ()
test_strings = do
    assert ("hello" <> " " <> "world" == "hello world") "concat"
    assert (show 42 == "42") "show int"
    assert (show True == "True") "show bool"
    assert (show [1, 2, 3] == "[1,2,3]") "show list"

-- ============================================================
-- Run all tests
-- ============================================================

main :: IO ()
main = do
    test_if
    test_let
    test_lambda
    test_case
    test_operators
    assert (id' 42 == 42) "id'"
    assert (fib 10 == 55) "fib 10"
    assert (classify (-5) == "negative") "classify neg"
    assert (classify 0 == "zero") "classify zero"
    assert (classify 5 == "positive") "classify pos"
    assert (circleArea 1.0 == 3.14159) "circle area"
    assert (Red == Red) "color eq"
    assert (Red /= Blue) "color neq"
    assert (show Green == "Green") "color show"
    assert (pointX (Point 3.0 4.0) == 3.0) "record access"
    assert (safeDivide 10 3 == Just 3) "safe divide ok"
    assert (safeDivide 10 0 == Nothing) "safe divide zero"
    assert (isWeekend Sat == True) "weekend sat"
    assert (isWeekend Mon == False) "weekday mon"
    assert (Low /= High) "priority neq"
    assert (Low < High) "priority ord"
    test_lists
    test_io
    test_laziness
    test_strings
