-- Test: negating the most negative machine integer at COMPILE TIME.
-- The constant folder reduces `-(-9223372036854775807 - 1)`: the inner
-- subtraction folds to i64::MIN, and its negation has no i64 result. The
-- folder used an unchecked negation there — a debug-build panic in the
-- compiler, a silent wrap in release — where every binary fold leaves an
-- overflowing result to the runtime. Now it does too: at type Integer the
-- runtime promotes to a bignum, at type Int it wraps as Lua does.

big :: Integer
big = -(-9223372036854775807 - 1)

wrapped :: Int
wrapped = -(-9223372036854775807 - 1)

main :: IO ()
main = do
    assert (show big == "9223372036854775808") "Integer negation of minBound promotes"
    assert (wrapped == -9223372036854775807 - 1) "Int negation of minBound wraps"
