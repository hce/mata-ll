-- Edge cases and feature interaction tests
-- Designed to find bugs in dark corners of the compiler

-- ============================================================
-- Empty / zero / boundary conditions
-- ============================================================

emptyList :: [Int]
emptyList = []

singletonList :: [Int]
singletonList = [42]

-- Zero-arg functions
unitFn :: Int
unitFn = 42

-- Negative numbers
negTest :: Int -> Int
negTest n = n * (-1)

describe :: Int -> String
describe 0 = "zero"
describe 1 = "one"
describe _ = "other"

main :: IO ()
main = do
    -- Empty list operations
    assert (length emptyList == 0) "empty length"
    assert (length singletonList == 1) "singleton length"
    assert (map (* 2) [] == ([] :: [Int])) "map empty"
    assert (filter (> 0) [] == ([] :: [Int])) "filter empty"
    assert (foldl (+) 0 [] == 0) "foldl empty"
    assert (reverse [] == ([] :: [Int])) "reverse empty"

    -- Negative numbers
    assert (negTest 5 == (-5)) "negate"
    assert (negTest (-3) == 3) "double negate"
    assert ((-1) + (-2) == (-3)) "neg arithmetic"

    -- ============================================================
    -- Partial application
    -- ============================================================
    let add = (+)
    assert (add 3 4 == 7) "op as function"
    let mul2 = (* 2)
    assert (mul2 5 == 10) "section"
    assert (map (* 3) [1, 2, 3] == [3, 6, 9]) "section in map"

    -- ============================================================
    -- Nested data structures
    -- ============================================================
    let nested = [[1, 2], [3, 4], [5]]
    assert (head (head nested) == 1) "nested list head"
    assert (length nested == 3) "nested list length"

    let maybeMaybe = Just (Just 42)
    assert (case maybeMaybe of { Just (Just x) -> x; _ -> 0 } == 42) "nested maybe"

    -- ============================================================
    -- Higher-order functions with complex args
    -- ============================================================
    assert (map (\x -> x * x) [1, 2, 3] == [1, 4, 9]) "map lambda"
    assert (filter (\x -> x `mod` 2 == 0) [1, 2, 3, 4] == [2, 4]) "filter lambda"
    assert (foldl (\acc x -> acc <> show x) "" [1, 2, 3] == "123") "foldl string build"

    -- ============================================================
    -- Case expression edge cases
    -- ============================================================

    -- Single-branch case
    let unwrap = \(Just x) -> x
    assert (unwrap (Just 99) == 99) "single branch case"

    -- Wildcard-only case
    let always42 = \_ -> 42
    assert (always42 "anything" == 42) "wildcard lambda"

    -- Nested case (inline to avoid multi-line continuation issue)
    assert ((case Just [1, 2, 3] of { Just (x:_) -> x; _ -> 0 }) == 1) "nested case match"

    -- ============================================================
    -- String edge cases
    -- ============================================================
    assert ("" <> "" == "") "empty concat"
    assert ("" <> "a" == "a") "empty left concat"
    assert ("a" <> "" == "a") "empty right concat"
    assert (show "" == "\"\"") "show empty string"
    assert (show 0 == "0") "show zero"
    assert (show (-42) == "-42") "show negative"

    -- ============================================================
    -- Boolean logic
    -- ============================================================
    assert ((True && True) == True) "and tt"
    assert ((True && False) == False) "and tf"
    assert ((False && True) == False) "and ft"
    assert ((False && False) == False) "and ff"
    assert ((True || False) == True) "or tf"
    assert ((False || False) == False) "or ff"
    assert (not True == False) "not true"
    assert (not False == True) "not false"

    -- ============================================================
    -- Arithmetic edge cases
    -- ============================================================
    assert (0 * 1000000 == 0) "mul zero"
    assert (1 * 1 == 1) "mul identity"
    assert (10 `div` 3 == 3) "integer div"
    assert (10 `mod` 3 == 1) "modulo"
    assert (0 `div` 5 == 0) "zero div"
    assert (0 `mod` 5 == 0) "zero mod"

    -- ============================================================
    -- Lazy evaluation edge cases
    -- ============================================================

    -- A let-bound bottom in an undemanded argument position is not forced:
    -- per-argument demand analysis suspends it despite Lua's eager argument
    -- evaluation. (This used to crash and was documented as a limitation.)
    let bottom = error "msg"
    assert (const 1 bottom == 1) "let-bound bottom not forced by const"

    -- Unused undefined (works because `undefined` is in concrete_vars)
    assert (const "safe" undefined == "safe") "unused undefined"

    -- Lazy list tail
    let xs = 1 : 2 : undefined
    assert (head xs == 1) "lazy head"
    assert (head (tail xs) == 2) "lazy head tail"

    -- ============================================================
    -- Tuple edge cases
    -- ============================================================
    let t2 = (1, 2)
    assert (fst t2 == 1) "tuple fst"
    assert (snd t2 == 2) "tuple snd"
    assert ((1, 2) == (1, 2)) "tuple eq"
    assert ((1, 2) /= (2, 1)) "tuple neq"

    -- ============================================================
    -- List comprehension edge cases
    -- ============================================================
    assert ([x | x <- []] == ([] :: [Int])) "comp empty src"
    assert ([x | x <- [1], False] == ([] :: [Int])) "comp false guard"
    assert ([x | x <- [1], True] == [1]) "comp true guard"
    assert (length [x | x <- [1,2,3,4,5], x > 3] == 2) "comp count"

    -- ============================================================
    -- Function composition and $
    -- ============================================================
    assert ((* 2) (3 + 4) == 14) "section apply"
    assert ((show $ 1 + 2) == "3") "dollar"

    -- ============================================================
    -- Multi-clause with different patterns
    -- ============================================================
    assert (describe 0 == "zero") "multi clause 0"
    assert (describe 99 == "other") "multi clause default"

    -- ============================================================
    -- Recursive data + operations
    -- ============================================================
    assert (length (take 5 [1,2,3,4,5,6,7]) == 5) "take"
    assert (head (take 3 [10,20,30,40]) == 10) "take head"

    pure ()
