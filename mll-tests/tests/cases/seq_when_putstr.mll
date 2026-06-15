-- Tests for seq, when, and putStr

-- ============================================================
-- seq: force first arg, return second
-- ============================================================

-- seq forces its first argument strictly
seqResult :: Integer
seqResult = seq (1 + 1) 42

-- seq with unit: force a value for its side-effect in pure code
seqChain :: Integer
seqChain = seq (2 * 3) (seq (4 + 5) 100)

-- ============================================================
-- when: conditional IO action
-- ============================================================

-- when True should perform the action
-- when False should skip it

-- ============================================================
-- putStr: write without newline
-- ============================================================

main :: IO ()
main = do
    -- seq basics
    assert (seqResult == 42) "seq returns second arg"
    assert (seqChain == 100) "seq chain"

    -- seq forces the first argument (if first arg is bottom, it should error)
    -- We can't test the error case easily, but we can verify it works with values
    assert (seq True "hello" == "hello") "seq with Bool"
    assert (seq [1, 2, 3] 99 == 99) "seq with list"
    assert (seq "forced" 0 == 0) "seq with string"

    -- seq in a let binding
    let val = seq (10 + 20) "computed"
    assert (val == "computed") "seq in let"

    -- when True performs the action
    when True (assert True "when True executes")

    -- when False does nothing (we verify by checking it doesn't crash
    -- and that subsequent code still runs)
    when False (putStrLn "this should not print")
    assert True "when False skipped"

    -- when with expressions
    let flag = 3 > 2
    when flag (assert True "when with expr")
    when (not flag) (putStrLn "this should not print either")
    assert True "when not flag skipped"

    -- putStr outputs without newline (we can at least verify it doesn't crash)
    putStr "hello"
    putStr " "
    putStr "world"
    putStrLn ""

    -- putStr with empty string
    putStr ""

    -- getArgs: returns empty list in test harness (no CLI args)
    args <- getArgs
    assert (args == ([] :: [String])) "getArgs empty"

    assert True "all seq/when/putStr tests passed"
