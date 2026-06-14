-- Comprehensive do-notation tests

-- Basic do sequencing
test_sequencing :: IO ()
test_sequencing = do
    let x = 1
    let y = 2
    assert (x + y == 3) "do let sequencing"

-- Bind
test_bind :: IO ()
test_bind = do
    x <- pure 42
    assert (x == 42) "do bind pure"
    y <- pure (x + 1)
    assert (y == 43) "do bind chain"

-- Multi-binding let
test_multi_let :: IO ()
test_multi_let = do
    let a = 10
        b = 20
        c = a + b
    assert (c == 30) "multi let"

-- Local function in do-let
test_let_function :: IO ()
test_let_function = do
    let double x = x * 2
    assert (double 5 == 10) "let function"
    let add x y = x + y
    assert (add 3 4 == 7) "let multi-param"

-- Nested let-in
test_nested :: IO ()
test_nested = do
    let r = 42
    let x = let a = r + 1 in a
    assert (x == 43) "nested let"

-- Do with if-then-else
test_if_in_do :: IO ()
test_if_in_do = do
    let flag = True
    if flag
        then assert True "if true branch"
        else assert False "should not reach"
    let flag2 = False
    if flag2
        then assert False "should not reach 2"
        else assert True "if false branch"

-- Do with case
test_case_in_do :: IO ()
test_case_in_do = do
    let x = Just 42
    case x of
        Just n -> assert (n == 42) "case in do Just"
        Nothing -> assert False "should not reach"

-- Do with let that shadows
test_shadowing :: IO ()
test_shadowing = do
    let x = 1
    assert (x == 1) "shadow before"
    let x = 2
    assert (x == 2) "shadow after"

-- IO actions are first-class (deferred)
test_action_value :: IO ()
test_action_value = do
    let action = putStrLn "."
    action
    action

-- Then (>>)
test_then :: IO ()
test_then = do
    pure ()
    pure ()
    assert True "then sequencing"

main :: IO ()
main = do
    test_sequencing
    test_bind
    test_multi_let
    test_let_function
    test_nested
    test_if_in_do
    test_case_in_do
    test_shadowing
    test_action_value
    test_then
