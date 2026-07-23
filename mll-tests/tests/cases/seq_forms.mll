-- `seq` must work in EVERY application shape, not just the prefix form, and
-- with identical semantics: force the FIRST argument to WHNF, then yield the
-- SECOND (its value is the second argument's value). Before the fix, only the
-- curried prefix `seq a b` was special-cased in codegen; the backtick infix,
-- partial application, and first-class forms fell through to a bare call of a
-- nonexistent global `seq` and crashed with "attempt to call a nil value".
--
-- This exercises all four shapes in BOTH directions — force works (a bottom in
-- the FIRST argument raises) and laziness is preserved (a bottom in the SECOND
-- argument is returned unforced, raising only when the consumer forces it) —
-- plus the WHNF-only property (a list's bottom head is not forced by seq) and
-- the proper-tail-call property for deep seq-strict recursion (prefix AND
-- backtick).

import Data.List (foldr)

inc :: Int -> Int
inc x = x + 1

-- Recursive: NOT an inline candidate, so `tri n` is a genuine thunked
-- application, not a folded constant.
tri :: Int -> Int
tri 0 = 0
tri n = n + tri (n - 1)

-- Ignores its second argument entirely (so a seq result passed here is never
-- consumed — its second-argument bottom must stay unforced).
keepFirst :: Int -> Int -> Int
keepFirst a _ = a

-- Deep seq-strict accumulators, one per inline form, to prove both keep the
-- proper tail call (constant stack). 1+2+...+N = N*(N+1)/2.
sumPrefix :: Int -> Int -> Int
sumPrefix 0 acc = acc
sumPrefix n acc = seq acc (sumPrefix (n - 1) (acc + n))

sumBacktick :: Int -> Int -> Int
sumBacktick 0 acc = acc
sumBacktick n acc = acc `seq` sumBacktick (n - 1) (acc + n)

main :: IO ()
main = do
    -- FORCE works, all four shapes: the first argument is evaluated, the
    -- second returned. (Thunked fields via `inc`/`tri`, never bare constants.)
    assert (seq (inc 40) 7 == 7) "prefix: force first, return second"
    assert ((inc 40 `seq` 7) == 7) "backtick infix: force first, return second"
    assert (let g = seq (tri 3) in g 8 == 8) "partial application: seq a awaiting b"
    assert (foldr seq 99 [inc 1, tri 3, inc 2] == 99) "first-class: foldr seq z"
    assert (map (seq (inc 0)) [10, 20, 30] == [10, 20, 30]) "first-class: map (seq x)"

    -- The value of `seq a b` is the value of `b` — a first-class `foldr seq`
    -- chain must fully evaluate to the base, not leave a residual thunk.
    assert (foldr seq (tri 4) [] == 10) "seq result is the second argument, forced"

    -- WHNF only: seq forces the second argument to WHNF, not deeper — a list's
    -- bottom head is not forced, so the spine can still be walked.
    assert (length (seq (inc 0) [errInt, errInt]) == 2) "seq forces second to WHNF only (lazy list heads)"

    -- LAZINESS preserved: a bottom in the SECOND argument is not forced when the
    -- seq result is discarded (here keepFirst ignores it). Must not raise.
    assert (keepFirst 5 (seq (inc 0) errInt) == 5) "second-argument bottom unforced when result discarded"

    -- ...but a consumed second argument IS evaluated.
    assert (seq (inc 0) (2 + 3) == 5) "consumed second argument evaluates"

    -- Deep seq-strict recursion runs in constant stack (proper tail call),
    -- prefix and backtick alike. 1..2000000 = 2000001000000.
    assert (sumPrefix 2000000 0 == 2000001000000) "prefix seq-strict deep tail recursion"
    assert (sumBacktick 2000000 0 == 2000001000000) "backtick seq-strict deep tail recursion"

    putStrLn "All seq application-form tests passed!"

-- A bottom used only where it must never be forced.
errInt :: Int
errInt = error "seq must not force this"
