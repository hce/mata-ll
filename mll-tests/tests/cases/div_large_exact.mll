-- lua-compat-skip: luajit
--   (integers > 2^53 are not representable on LuaJIT's doubles: the
--   literals below are rounded before any arithmetic runs, so exactness
--   cannot hold there — a documented limitation, see
--   doc/articles/CAVEATS.md. The contract holds on 64-bit-integer hosts.)
--
-- Regression (Finding 3, part 2 — 64-BIT-INTEGER TARGET ONLY, i.e. the
-- Lua 5.4 backend; on LuaJIT (5.1 doubles) Integers above 2^53 are not
-- representable at all, so this file is expected to remain a documented
-- limitation there):
--
-- `div` compiles to `math.floor(a / b)` — FLOAT division — so any
-- operand above 2^53 = 9007199254740992 loses low bits in the float
-- mantissa before the divide even happens:
--   9007199254740993 `div` 1  currently returns 9007199254740992
--   4611686018427387905 `div` 3 is off by 85
-- Int division must be EXACT over the full 64-bit range.
--
-- Every expected value below was verified by hand:
--   2^53+1  = 9007199254740993
--     div 1 = 9007199254740993                 mod 1 = 0
--     div 2 = 4503599627370496 (= 2^52)        mod 2 = 1
--     div 3 = 3002399751580331 (x3 = 2^53+1)   mod 3 = 0
--   2^62+1  = 4611686018427387905
--     div 3 = 1537228672809129301 (x3 = 4611686018427387903)   mod 3 = 2
--   2^63-1  = 9223372036854775807 (maxBound Int64)
--     div 2 = 4611686018427387903 (= 2^62-1)   mod 2 = 1
--     div 3 = 3074457345618258602 (x3 = 9223372036854775806)   mod 3 = 1
--   10^18+3 = 1000000000000000003
--     div 10^9 = 1000000000                    mod 10^9 = 3
--   -(2^53+1) div 2 = -4503599627370497 (floor) mod 2 = 1
--
-- The runtime path goes through opaque functions (dz/mz) so constant
-- folding cannot compute the answers at compile time and mask the
-- runtime bug; the literal infix forms at the end additionally pin the
-- folded path (fold.rs works in i64, which is exact for positive
-- divisors).

dz :: Int -> Int -> Int
dz a b = a `div` b

mz :: Int -> Int -> Int
mz a b = a `mod` b

law :: Int -> Int -> Bool
law a b = dz a b * b + mz a b == a

main :: IO ()
main = do
    -- 2^53+1: the smallest odd integer a double cannot represent.
    assert (dz 9007199254740993 1 == 9007199254740993) "2^53+1 div 1 exact"
    assert (mz 9007199254740993 1 == 0)                "2^53+1 mod 1"
    assert (dz 9007199254740993 2 == 4503599627370496) "2^53+1 div 2 exact"
    assert (mz 9007199254740993 2 == 1)                "2^53+1 mod 2 sees the low bit"
    assert (dz 9007199254740993 3 == 3002399751580331) "2^53+1 div 3 exact"
    assert (mz 9007199254740993 3 == 0)                "2^53+1 mod 3"

    -- 2^62+1: the finding's off-by-85 witness.
    assert (dz 4611686018427387905 3 == 1537228672809129301) "2^62+1 div 3 exact"
    assert (mz 4611686018427387905 3 == 2)                   "2^62+1 mod 3"
    assert (dz 4611686018427387905 1 == 4611686018427387905) "2^62+1 div 1 identity"

    -- maxBound :: Int64.
    assert (dz 9223372036854775807 1 == 9223372036854775807) "int64 max div 1 identity"
    assert (dz 9223372036854775807 2 == 4611686018427387903) "int64 max div 2 exact"
    assert (mz 9223372036854775807 2 == 1)                   "int64 max mod 2"
    assert (dz 9223372036854775807 3 == 3074457345618258602) "int64 max div 3 exact"
    assert (mz 9223372036854775807 3 == 1)                   "int64 max mod 3"

    -- Large dividend, large divisor.
    assert (dz 1000000000000000003 1000000000 == 1000000000) "10^18+3 div 10^9"
    assert (mz 1000000000000000003 1000000000 == 3)          "10^18+3 mod 10^9 keeps the +3"

    -- Negative large dividend: floor semantics must survive at magnitude.
    assert (dz (-9007199254740993) 2 == (-4503599627370497)) "-(2^53+1) div 2 floors"
    assert (mz (-9007199254740993) 2 == 1)                   "-(2^53+1) mod 2 divisor sign"

    -- The div/mod law at 64-bit magnitudes.
    assert (law 9007199254740993 3)       "law 2^53+1 / 3"
    assert (law 4611686018427387905 3)    "law 2^62+1 / 3"
    assert (law 9223372036854775807 7)    "law int64 max / 7"
    assert (law (-9007199254740993) 2)    "law negative large / 2"

    -- Folded path: literal operands (fold.rs, i64 arithmetic).
    assert (9007199254740993 `div` 1 == 9007199254740993)        "literal 2^53+1 div 1"
    assert (4611686018427387905 `div` 3 == 1537228672809129301)  "literal 2^62+1 div 3"
    assert (4611686018427387905 `mod` 3 == 2)                    "literal 2^62+1 mod 3"

    putStrLn "ok"
