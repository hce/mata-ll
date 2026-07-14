-- `div` and `mod` must work in EVERY application shape, not just the backtick
-- infix. Before the fix, only ``a `div` b`` / ``a `mod` b`` (and their backtick
-- sections) were lowered; the PREFIX form (`div 7 2`), PARTIAL application
-- (`map (div 10) xs`), and any FIRST-CLASS / higher-order use referenced a bare
-- Lua global `div`/`mod` that does not exist and crashed at runtime with
-- "attempt to call a nil value" (audit finding 4).
--
-- The prefix/partial/first-class forms now resolve to the runtime wrappers
-- __mll_div_fn / __mll_mod_fn, which FORCE both arguments to WHNF and then run
-- the same strict core the backtick form uses — so the result is identical in
-- every shape, including floor semantics with negative operands and the
-- zero-divisor error. The wrapper's forcing is what makes it safe when a caller
-- (e.g. `map`) hands the operator unforced (thunk) arguments.

import Data.List (foldr)

-- Recursive, NOT inline-eligible, so `slow n` is a genuine thunked application
-- (a value that must be forced), never a folded constant. Passing `slow k` as an
-- operand to a first-class `div`/`mod` proves the wrapper forces its arguments.
slow :: Integer -> Integer
slow 0 = 0
slow n = 1 + slow (n - 1)

-- Higher-order: takes a binary operator as a first-class value and applies it.
-- `applyBin div a b` forces `div` to a value and calls it — the first-class path.
applyBin :: (Integer -> Integer -> Integer) -> Integer -> Integer -> Integer
applyBin f a b = f a b

main :: IO ()
main = do
    -- PREFIX full application: must equal the backtick form.
    assert (div 7 2 == 3) "prefix div"
    assert (mod 7 2 == 1) "prefix mod"
    assert (div 7 2 == (7 `div` 2)) "prefix div agrees with backtick"
    assert (mod 7 2 == (7 `mod` 2)) "prefix mod agrees with backtick"

    -- Floor semantics with negative operands survive the wrapper (same core):
    -- quotient rounds toward -inf, remainder takes the sign of the divisor.
    assert (div 7 (0 - 2) == (0 - 4)) "prefix div floors toward negative infinity"
    assert (mod 7 (0 - 2) == (0 - 1)) "prefix mod takes divisor's sign"
    assert (div (0 - 7) 2 == (0 - 4)) "prefix div, negative dividend"
    assert (mod (0 - 7) 2 == 1) "prefix mod, negative dividend"

    -- PARTIAL application through `map`: `div 240` / `mod 240` are closures that
    -- map applies to each (thunked) list element. This is the exact form the
    -- CAVEATS entry called out; it used to crash.
    assert (map (div 240) [2, 3, 4, 5] == [120, 80, 60, 48]) "partial div via map"
    assert (map (mod 10) [3, 4, 7, 9] == [1, 2, 3, 1]) "partial mod via map"

    -- FIRST-CLASS: pass `div` / `mod` as an argument, and feed it THUNKED
    -- operands (`slow k`). If the wrapper did not force, this crashes with
    -- "arithmetic on a table/function value"; if the operator resolved to a
    -- bare nil global, it crashes with "attempt to call a nil value".
    assert (applyBin div (slow 240) (slow 3) == 80) "first-class div, thunked operands"
    assert (applyBin mod (slow 10) (slow 3) == 1) "first-class mod, thunked operands"

    -- FIRST-CLASS through foldr: right fold with div as the combining op.
    -- foldr div 2 [16, 48] = div 16 (div 48 2) = div 16 24 = 0.
    assert (foldr div 2 [16, 48] == 0) "first-class div via foldr"

    -- The zero-divisor error must raise through the prefix/first-class path too,
    -- not just the backtick form. `seq` demands the quotient inside the tried
    -- action so the test does not depend on how eagerly IO evaluates.
    r1 <- try (div 1 0 `seq` pure ())
    case r1 of
        Left _   -> putStrLn "prefix div by zero raises"
        Right () -> error "prefix `div 1 0` must raise, not return"

    r2 <- try (applyBin mod 1 0 `seq` pure ())
    case r2 of
        Left _   -> putStrLn "first-class mod by zero raises"
        Right () -> error "first-class `mod 1 0` must raise, not return"

    putStrLn "All div/mod application-form tests passed!"
