-- Regression test: do-block evaluation order and let/bind interactions
-- Ensures let flattening doesn't break IO sequencing or scoping

main :: IO ()
main = do
    -- Test 1: multiple lets interleaved with IO
    let a = 1
    let b = a + 1
    let c = b + 1
    assert (c == 3) "sequential lets"

    -- Test 2: let shadowing in do
    let x = 10
    let y = x + 1
    let x = 20
    let z = x + y
    assert (z == 31) "let shadowing"

    -- Test 3: long chain of lets between IO actions
    let s1 = 1
    let s2 = s1 + 1
    let s3 = s2 + 1
    let s4 = s3 + 1
    let s5 = s4 + 1
    let s6 = s5 + 1
    let s7 = s6 + 1
    let s8 = s7 + 1
    let s9 = s8 + 1
    let s10 = s9 + 1
    assert (s10 == 10) "long let chain"
    putStrLn "phase 1 ok"

    -- Test 4: string building with lets
    let greeting = "hello"
    let space = " "
    let name = "world"
    let msg = greeting <> space <> name
    assert (msg == "hello world") "string lets"

    -- Test 5: list operations in lets
    let xs = [1, 2, 3, 4, 5]
    let total = foldl (+) 0 xs
    let doubled = map (* 2) xs
    let dtotal = foldl (+) 0 doubled
    assert (total == 15) "sum original"
    assert (dtotal == 30) "sum doubled"

    -- Test 6: lets with conditionals
    let n = 5
    let label = if n > 0 then "positive" else "non-positive"
    assert (label == "positive") "conditional let"

    -- Test 7: IO action between let groups
    putStrLn "phase 2 ok"
    let p = 100
    let q = p + 50
    assert (q == 150) "post-IO lets"

    -- Test 8: lets depending on each other across IO
    let m1 = 42
    putStrLn "phase 3 ok"
    let m2 = m1 + 1
    assert (m2 == 43) "let across IO"

    -- Test 9: bind with pure (was a known bug: thunk not unwrapped)
    v1 <- pure (99 :: Int)
    assert (v1 == 99) "bind pure integer"

    -- Test 10: bind with return
    v2 <- return (7 :: Int)
    assert (v2 == 7) "bind return integer"

    -- Test 11: bind pure then use in let
    v3 <- pure (50 :: Int)
    let v4 = v3 + 10
    assert (v4 == 60) "bind pure then let"

    -- Test 12: interleaved bind and let
    let a1 = 100
    b1 <- pure (a1 + 1)
    let c1 = b1 + 1
    d1 <- pure (c1 + 1)
    assert (d1 == 103) "interleaved bind let"

    -- Test 13: bind pure with expression
    x1 <- pure (3 * 4 + 1)
    assert (x1 == 13) "bind pure expr"

    putStrLn "ok"
