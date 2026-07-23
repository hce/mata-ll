-- IO action semantics: actions are descriptions of effects,
-- not the effects themselves. Only >>= and >> may perform them.

-- Helper: a mutable counter via STArray to track execution order
countExecutions :: STArray s -> Int -> ST s ()
countExecutions arr idx = do
    n <- readSTArray arr idx
    writeSTArray arr idx (n + 1)

-- Test 1: let-bound IO actions are not performed at binding site
test_let_deferred :: IO ()
test_let_deferred = do
    -- This should NOT print "boom" — the action is stored, not performed
    let action = putStrLn "boom"
    -- Only this prints
    putStrLn "."
    -- 'action' is never used, so "boom" should never appear

-- Test 2: pure/return wraps a value, doesn't perform anything
test_pure_wraps :: IO ()
test_pure_wraps = do
    let x = pure 42
    -- x is an IO action holding 42, not the value 42 itself
    val <- x
    assert (val == 42) "pure wraps value"

-- Test 3: actions in if/then/else — only the chosen branch executes
test_conditional_action :: IO ()
test_conditional_action = do
    let flag = True
    let a = putStrLn "SHOULD NOT PRINT"
    let b = pure ()
    if flag then b else a
    putStrLn "."

-- Test 4: ST actions in let bindings are deferred
test_st_let_deferred :: IO ()
test_st_let_deferred = do
    let result = runST (do
            arr <- newSTArray 2 0
            -- Store actions in let bindings — should NOT execute yet
            let write1 = writeSTArray arr 0 42
                write2 = writeSTArray arr 1 99
            -- Only perform write1, not write2
            write1
            v0 <- readSTArray arr 0
            v1 <- readSTArray arr 1
            pure (v0, v1))
    assert (fst result == 42) "ST let: write1 executed"
    assert (snd result == 0) "ST let: write2 not executed"

-- Test 5: same action can be performed multiple times
test_action_reuse :: IO ()
test_action_reuse = do
    let result = runST (do
            arr <- newSTArray 1 0
            let inc = countExecutions arr 0
            inc
            inc
            inc
            readSTArray arr 0)
    assert (result == 3) "action reuse: ran 3 times"

-- Test 6: actions passed to pure functions
test_action_passed_to_pure :: IO ()
test_action_passed_to_pure = do
    let result = runST (do
            arr <- newSTArray 1 0
            let a = writeSTArray arr 0 10
                b = writeSTArray arr 0 20
            -- Pure function chooses which action to run
            let chosen = if True then a else b
            chosen
            readSTArray arr 0)
    assert (result == 10) "pure function chose action a"

-- Test 7: ordering of effects
test_effect_ordering :: IO ()
test_effect_ordering = do
    let result = runST (do
            arr <- newSTArray 1 0
            writeSTArray arr 0 1
            writeSTArray arr 0 2
            writeSTArray arr 0 3
            readSTArray arr 0)
    assert (result == 3) "effects in order: last write wins"

main :: IO ()
main = do
    test_let_deferred
    test_pure_wraps
    test_conditional_action
    test_st_let_deferred
    test_action_reuse
    test_action_passed_to_pure
    test_effect_ordering
