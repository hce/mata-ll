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

    -- Known limitation: let-bound bottoms passed to non-strict positions
    -- ARE eagerly evaluated at call sites (Lua evaluates all args).
    -- `let bottom = error "boom"; const 1 bottom` will crash.
    -- Use wildcard patterns in the callee to avoid this.

    pure ()
