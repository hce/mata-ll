-- Regression: call-site inlining must not duplicate argument work.
-- `sq x = x * x` is an inline candidate; before the fix, applying it to a
-- non-trivial argument substituted that argument at BOTH occurrences of x,
-- so `sq (nfib 20)` evaluated the call twice (measured 2x wall time) — a
-- sharing loss GHC's inliner never allows. The fix declines substitution
-- for a multiply-occurring parameter unless the argument is trivial
-- (variable/literal), falling back to the ordinary call, which evaluates
-- the argument once. This case pins the observable behavior of every gate
-- outcome; the companion Rust test (inlining_preserves_argument_sharing in
-- run_mll.rs) pins the emitted code.

sq :: Int -> Int
sq x = x * x

-- Both tuple fields mention x: occurrence count 2 through a lazy position.
dup :: Int -> (Int, Int)
dup x = (x, x)

-- x occurs once in EACH branch of an if — the branches are exclusive at
-- runtime, so the work-duplication count is their maximum (1) and a
-- non-trivial argument may still be substituted here, as in GHC.
pick :: Bool -> Int -> Int
pick b x = if b then x + 1 else x - 1

-- The parameter never occurs: a non-trivial argument is dropped, not run.
constFive :: Int -> Int
constFive _ = 5

sumTo :: Int -> Int
sumTo n = if n <= 0 then 0 else n + sumTo (n - 1)

main :: IO ()
main = do
    -- Declined inline (non-trivial arg, param used twice): ordinary call.
    assert (sq (sumTo 10) == 3025) "sq of a call computes once-shared"
    -- Still-inlined shapes: trivial args into a multiply-used param.
    let y = 6
    assert (sq y == 36) "sq of a variable stays inlined"
    assert (sq 9 == 81) "sq of a literal stays inlined"
    -- Branch-exclusive occurrences admit a non-trivial argument.
    assert (pick True (sumTo 4) == 11) "if-branch occurrences count as one (then)"
    assert (pick False (sumTo 4) == 9) "if-branch occurrences count as one (else)"
    -- Laziness across the declined path: dup suspends its argument per
    -- field, and an unused parameter never runs its argument at all.
    assert (fst (dup (sumTo 5)) == 15) "tuple-field sharing case computes"
    assert (constFive (error "unreached") == 5) "unused param still drops bottom"
    pure ()
