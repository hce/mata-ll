-- Regression test: nested function calls must preserve sharing
-- f(f(f(x))) must give the same result as let-based equivalent.
-- (Bug: is_cheap_arg treated nested calls as cheap, breaking thunking)

-- Simple increment
inc :: Int -> Int
inc x = x + 1

-- Conditional function (has branching, so thunking matters)
clamp :: Int -> Int
clamp x = if x > 10 then 10 else x

-- Stateful-looking: result depends on which branch was taken
decOrKeep :: Int -> Int
decOrKeep x = if x > 0 then x - 1 else x

-- Test: nested form must equal let form
nestedInc :: Int -> Int
nestedInc x = inc (inc (inc x))

letInc :: Int -> Int
letInc x =
    let a = inc x
        b = inc a
    in inc b

nestedClamp :: Int -> Int
nestedClamp x = clamp (clamp (clamp x))

letClamp :: Int -> Int
letClamp x =
    let a = clamp x
        b = clamp a
    in clamp b

nestedDec :: Int -> Int
nestedDec x = decOrKeep (decOrKeep (decOrKeep x))

letDec :: Int -> Int
letDec x =
    let a = decOrKeep x
        b = decOrKeep a
    in decOrKeep b

-- Deeply nested (5 levels)
fiveInc :: Int -> Int
fiveInc x = inc (inc (inc (inc (inc x))))

-- Mixed function nesting
mixedNest :: Int -> Int
mixedNest x = clamp (inc (inc (inc x)))

main :: IO ()
main = do
    -- Basic nesting
    assert (nestedInc 0 == 3) "nested inc 0"
    assert (letInc 0 == 3) "let inc 0"
    assert (nestedInc 0 == letInc 0) "nested == let (inc)"

    -- Conditional nesting
    assert (nestedClamp 5 == 5) "nested clamp 5"
    assert (letClamp 5 == 5) "let clamp 5"
    assert (nestedClamp 15 == 10) "nested clamp 15"
    assert (letClamp 15 == 10) "let clamp 15"
    assert (nestedClamp 15 == letClamp 15) "nested == let (clamp)"

    -- Decrement nesting
    assert (nestedDec 5 == 2) "nested dec 5"
    assert (letDec 5 == 2) "let dec 5"
    assert (nestedDec 1 == 0) "nested dec 1"
    assert (letDec 1 == 0) "let dec 1"
    assert (nestedDec 0 == 0) "nested dec 0 (idempotent)"
    assert (nestedDec 0 == letDec 0) "nested == let (dec 0)"

    -- Deep nesting
    assert (fiveInc 0 == 5) "five nested inc"

    -- Mixed
    assert (mixedNest 5 == 8) "mixed nest 5"
    assert (mixedNest 100 == 10) "mixed nest 100 (clamped)"

    putStrLn "All nested call tests passed!"
