-- Regression for B4: composing operator sections with `.` (and applying
-- them via `$`) emitted a bare Lua function literal in call position —
-- `function(_sec) ... end(x)` — which Lua's grammar rejects. Callees that
-- are function literals must be parenthesized: `(function() ... end)(x)`.

inc :: Integer -> Integer
inc x = x + 1

dbl :: Integer -> Integer
dbl x = x * 2

-- Inline-path coverage: `app` and `comp` are small pure single-clause
-- functions, so codegen inlines them at exact-arity call sites and their
-- bodies go through the substituting emitter (gen_expr_subst), which had
-- the same bug for `$` and emitted `.` as an invalid Lua infix operator.
app :: (Integer -> Integer) -> Integer -> Integer
app f x = f $ x

comp :: (Integer -> Integer) -> (Integer -> Integer) -> Integer -> Integer
comp f g = f . g

addN :: Integer -> (Integer -> Integer)
addN n = (+n)

main :: IO ()
main = do
    -- Section-composition chains, both operand orders
    assert (((+1) . (*2)) 5 == 11) "section . section"
    assert (((*2) . (+1)) 5 == 12) "section . section, other order"

    -- Mixed named/section operands
    assert ((inc . (*2)) 5 == 11) "named . section"
    assert (((*2) . inc) 5 == 12) "section . named"

    -- Higher-order use of a section composition
    assert (map ((+1) . (*2)) [1, 2, 3] == [3, 5, 7]) "map over section composition"

    -- Longer chain and a lambda operand
    assert (((+1) . (*2) . (+3)) 5 == 17) "three-stage section chain"
    assert (((\x -> x - 1) . (*2)) 5 == 9) "lambda . section"

    -- $ with a function-literal callee
    assert (((+1) $ 5) == 6) "section $ arg"
    assert (((\x -> x * 3) $ 4) == 12) "lambda $ arg"

    -- Guards: named-function composition and bare section application
    -- must keep working (and keep emitting unwrapped callees)
    assert ((inc . dbl) 5 == 11) "named . named"
    assert ((+1) 5 == 6) "bare section application"

    -- A section produced by a function, then applied
    -- (top-level, not let-bound: two-step application of a let-bound
    -- curried lambda is a separate pre-existing bug, present on HEAD
    -- even without sections)
    assert ((addN 10) 5 == 15) "section returned then applied"

    -- Inline (substituting) emitter: `$` with a section substituted for
    -- the callee, and a `.` body with named and section replacements
    assert (app (+1) 5 == 6) "inlined $ with section arg"
    let h = comp inc dbl
    assert (h 5 == 11) "inlined composition, named args"
    let h2 = comp (+1) (*2)
    assert (h2 5 == 11) "inlined composition, section args"
