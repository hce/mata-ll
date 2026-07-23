-- Regression test: let scoping rules in do blocks
-- Ensures that let bindings are visible in subsequent statements
-- and interact correctly with IO actions and bind operations

main :: IO ()
main = do
    -- Test 1: let capture semantics - f captures x=10, rebinding x doesn't change f
    let x = 10
    let f = x + 1
    let x = 20
    let z = x + f
    assert (z == 31) "capture test"

    -- Test 2: trailing lets before final IO
    let a = 1
    let b = 2
    let c = 3
    assert (a + b + c == 6) "trailing lets"

    -- Test 3: let with complex expressions
    let xs = [1, 2, 3, 4, 5]
    let total = foldl (+) 0 xs
    let doubled = map (* 2) xs
    let dtotal = foldl (+) 0 doubled
    assert (total == 15) "sum original"
    assert (dtotal == 30) "sum doubled"

    -- Test 4: many lets between IO actions
    let h = 100
    let i = h + 1
    let j = i + 1
    let k = j + 1
    let l = k + 1
    let m = l + 1
    assert (m == 105) "many lets"

    -- Test 5: let in do with conditionals
    let n = 5
    let label = if n > 0 then "positive" else "non-positive"
    let result = label <> ": " <> show (n * 2)
    assert (result == "positive: 10") "conditional let"

    -- Test 6: negative case
    let neg = -3
    let nlabel = if neg > 0 then "positive" else "non-positive"
    let nresult = nlabel <> ": " <> show (neg * 2)
    assert (nresult == "non-positive: -6") "conditional negative"

    -- Test 7: let with function application
    let nums = [10, 20, 30]
    let len = length nums
    let hd = head nums
    assert (len == 3) "length in let"
    assert (hd == 10) "head in let"

    -- Test 8: let with where-like chaining
    let base = 2
    let step1 = base * base
    let step2 = step1 * step1
    let step3 = step2 * step2
    assert (step3 == 256) "power chain"

    -- Test 9: bind scoping - bound var visible in subsequent lets
    v <- pure (42 :: Int)
    let w = v + 8
    assert (w == 50) "bind then let"

    -- Test 10: bind shadowing - rebinding via <- shadows previous let
    let q = 10
    q <- pure (20 :: Int)
    assert (q == 20) "bind shadows let"

    -- Test 11: let sees previous bind
    r1 <- pure (5 :: Int)
    r2 <- pure (r1 * 2)
    let r3 = r1 + r2
    assert (r3 == 15) "let sees binds"

    -- Test 12: bind with string
    greeting <- pure "hello"
    let msg = greeting <> " world"
    assert (msg == "hello world") "bind string"

    -- Test 13: bind between IO actions preserves scope
    putStrLn "scope check"
    p <- pure (77 :: Int)
    putStrLn "scope check 2"
    let p2 = p + 3
    assert (p2 == 80) "bind across IO"

    putStrLn "ok"
