-- Regression: `div` must be integer-exact and a zero divisor must RAISE.
--
-- `div` used to be emitted as `math.floor(a / b)` — float division — so:
--   * `1 `div` 0` produced `inf`, a float silently flowing on as if it were
--     an Int (mod by zero already raised, div did not);
--   * quotients of integers beyond 2^53 were wrong (float mantissa runs out:
--     4611686018427387905 `div` 3 came out 85 too small).
-- The runtime now routes div/mod through __mll_div/__mll_mod, which raise a
-- plain-language "divide by zero" error and use Lua 5.3+'s native integer
-- floor division `//` (exact over the full 64-bit range). This test runs on
-- the embedded Lua 5.4, where exactness is guaranteed; on a LuaJIT host all
-- numbers are doubles, so >2^53 exactness is a documented host limitation
-- (doc/articles/CAVEATS.md) — the zero-divisor error raises there too.

d :: Int -> Int -> Int
d a b = a `div` b

m :: Int -> Int -> Int
m a b = a `mod` b

sub :: Int -> Int -> Int
sub a b = a - b

-- True on hosts whose Int is a real 64-bit integer (Lua 5.3+); False on
-- double-only hosts (LuaJIT / 5.1-5.2), where 2^53+1 and 2^53 are the SAME
-- number, so their difference is 0. Computed through `sub` on the host so
-- the compile-time constant folder (which works in exact 64-bit integers)
-- cannot pre-decide it.
hostHasIntegers :: Bool
hostHasIntegers = sub 9007199254740993 9007199254740992 == 1

main :: IO ()
main = do
    -- div by zero raises (was: inf). `seq` demands the quotient inside the
    -- tried action, so the test does not depend on how eagerly IO evaluates.
    r1 <- try (d 1 0 `seq` pure ())
    case r1 of
        Left _   -> putStrLn "div by zero raises"
        Right () -> error "1 `div` 0 must raise, not return"

    -- ...also when the operands are literals (the folder must NOT fold a
    -- zero divisor into anything; the runtime raises).
    r2 <- try ((1 `div` 0) `seq` pure ())
    case r2 of
        Left _   -> putStrLn "literal div by zero raises"
        Right () -> error "literal 1 `div` 0 must raise, not return"

    -- mod by zero raises with the same clear error on every host (on LuaJIT
    -- the bare `%` used to yield nan silently).
    r3 <- try (m 1 0 `seq` pure ())
    case r3 of
        Left _   -> putStrLn "mod by zero raises"
        Right () -> error "1 `mod` 0 must raise, not return"

    -- Int-exact division beyond the 2^53 float mantissa. This is the
    -- CONTRACT on integer hosts (Lua 5.3+, including the embedded 5.4 the
    -- cargo test suite always runs on — the branch is always taken there).
    -- On a double-only host (LuaJIT) the literals themselves are already
    -- rounded before div ever runs, so exactness out here is a documented
    -- host limitation (CAVEATS.md), not a division property to assert.
    if hostHasIntegers
      then do
        assert (d 4611686018427387905 3 == 1537228672809129301) "big div is exact"
        assert (m 4611686018427387905 3 == 2) "big mod is exact"
        assert (d 9007199254740993 2 == 4503599627370496) "div just past 2^53 is exact"
        -- The div/mod law holds out there too.
        assert (4611686018427387905 == 3 * d 4611686018427387905 3 + m 4611686018427387905 3)
            "div/mod law at 62 bits"
      else putStrLn "host numbers are doubles: >2^53 exactness not asserted (see CAVEATS.md)"

    -- Within the double-exact range the results are exact on EVERY host.
    assert (d 9007199254740992 2 == 4503599627370496) "div at 2^53 is exact everywhere"
    assert (d 123456789012345 1000 == 123456789012) "mid-range div exact everywhere"
    assert (m 123456789012345 1000 == 345) "mid-range mod exact everywhere"

    putStrLn "div exactness and zero-divisor ok"
