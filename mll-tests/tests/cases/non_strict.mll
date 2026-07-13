-- Non-strict evaluation: bottom/undefined must not be evaluated
-- unless actually forced.

main :: IO ()
main = do
    -- undefined in a let binding is not forced
    let x = undefined
    assert (const "safe" x == "safe") "const discards bottom"

    -- undefined in a list that is never reached
    let xs = 1 : 2 : undefined
    assert (head xs == 1) "head of partial list"

    -- A let-bound bottom passed in a position the callee never demands is
    -- not evaluated: per-argument demand analysis suspends it, even though
    -- Lua itself evaluates call arguments eagerly. (This used to crash.)
    let bottom = error "boom"
    assert (const 1 bottom == 1) "let-bound bottom discarded by const"

    pure ()
