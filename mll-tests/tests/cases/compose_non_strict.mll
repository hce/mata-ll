-- Regression: function composition (`.`) must respect the non-strict
-- semantics of the outer function. `(f . g) x` is `f (g x)`; when `f` does
-- not force its argument, `g x` must not be evaluated — even when `g` itself
-- is strict — so any bottom in `g x` is never reached.
--
-- Before the fix, the `.` codegen emitted `f(g(x))` with `g(x)` eager, so
-- `(ignore . add1) (error "boom")` ran `add1 (error …)`, forced the error,
-- and crashed where GHC returns 5. This closes that hole so the eagerness
-- contract (bottom is never forced by a consumer that does not demand it)
-- holds through composition, not only through direct application.

ignore :: Integer -> Integer
ignore _ = 5

add1 :: Integer -> Integer
add1 z = z + 1

dbl :: Integer -> Integer
dbl z = z * 2

main :: IO ()
main = do
    -- Non-strict outer function: the bottom in `add1 (error …)` is discarded.
    assert (ignore (add1 (error "unreached")) == 5) "compose: direct nesting discards bottom"
    assert ((ignore . add1) (error "unreached") == 5) "compose: (.) discards bottom"
    -- Chained composition, non-strict outermost function.
    assert ((ignore . add1 . dbl) (error "unreached") == 5) "compose: chained (.) discards bottom"

    -- Strict compositions still compute the right value (no over-laziness).
    assert ((add1 . add1) 10 == 12) "compose: strict chain computes"
    assert ((dbl . add1) 5 == 12) "compose: mixed chain computes"
    assert (map (add1 . dbl) [1, 2, 3] == [3, 5, 7]) "compose: inside map computes"

    pure ()
