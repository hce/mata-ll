-- Regression test: point-free (zero-pattern) top-level *function* definitions
-- that forward-reference later bindings. The value-binding codegen path used
-- to capture such a definition regardless of its arity, emitting an eager
-- `__mll_fn[f] = __mll_fn[g]` copy at module-load time -- but the referent's
-- slot was still nil (defined later), so any call crashed with
-- "attempt to call a nil value". The fix routes function-typed point-free
-- definitions (eta_count > 0) to the eta-expanding function branch, which
-- forwards to the referent at call time.

-- A bare alias to a function defined BELOW.
inc :: Int -> Int
inc = bump

bump :: Int -> Int
bump n = n + 1

-- A point-free precedence-ladder chain, like the BASIC expression parser:
-- each rung is defined point-free in terms of the next, which appears later.
classify :: Int -> String
classify = stageA

stageA :: Int -> String
stageA = stageB

-- Partial application forward-referencing a later binding (the other shape).
stageB :: Int -> String
stageB = label "n="

label :: String -> Int -> String
label prefix n = prefix <> show n

-- A point-free predicate aliasing a function defined below.
nonNeg :: Int -> Bool
nonNeg = atLeastZero

atLeastZero :: Int -> Bool
atLeastZero n = n >= 0

main :: IO ()
main = do
    assert (inc 41 == 42) "bare alias to later function"
    assert (classify 7 == "n=7") "point-free chain through later bindings"
    assert (stageB 9 == "n=9") "partial-application alias forward ref"
    assert (nonNeg 4) "point-free predicate forward ref"
    assert (not (nonNeg (0 - 1))) "point-free predicate forward ref (false case)"
    putStrLn "pointfree_caf ok"
