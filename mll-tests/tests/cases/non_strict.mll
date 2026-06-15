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

    -- let-bound bottom passed to non-strict position
    let bottom = error "boom"
    assert (const 1 bottom == 1) "const discards let-bound bottom"
    assert (const "safe" undefined == "safe") "const discards undefined"

    pure ()
