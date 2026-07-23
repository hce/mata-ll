-- lua-compat-skip: luajit
--   (integers > 2^53 are not representable on LuaJIT's doubles: the
--   literals below are rounded before any arithmetic runs, so exactness
--   cannot hold there — a documented limitation, see
--   doc/articles/CAVEATS.md. The contract holds on 64-bit-integer hosts.)
--
-- Regression (Finding 3, interaction — 64-BIT-INTEGER TARGET ONLY):
-- a large `div` result must be a REAL integer downstream, not a float
-- masquerading as one. A float sneaks through `==` against a rounded
-- literal, but it is observable the moment the value flows onward:
--   * `show` renders Lua floats as "9.007199254741e+15" (or a rounded
--     integer if math.floor re-integerized it) — never the exact digits;
--   * further arithmetic re-rounds at every step (float 2^53 + 1 == 2^53).
-- So these tests thread quotients into show, +, *, comparison chains,
-- and list folds.
--
-- Hand-verified values:
--   q1 = 9007199254740993 `div` 1 = 9007199254740993;  q1+1 = 9007199254740994
--   q2 = 4611686018427387905 `div` 3 = 1537228672809129301
--        q2*3 = 4611686018427387903;  q2*3+2 = 4611686018427387905
--   q3 = 9223372036854775807 `div` 2 = 4611686018427387903 = q2*3

dz :: Int -> Int -> Int
dz a b = a `div` b

mz :: Int -> Int -> Int
mz a b = a `mod` b

sumList :: [Int] -> Int
sumList xs = foldl (+) 0 xs

main :: IO ()
main = do
    let q1 = dz 9007199254740993 1
    -- show must reproduce the exact decimal digits.
    assert (show q1 == "9007199254740993")   "show of 2^53+1 quotient is exact"
    -- float 2^53 + 1 rounds back to 2^53; integers must not.
    assert (q1 + 1 == 9007199254740994)      "quotient + 1 lands on 2^53+2"
    assert (q1 - 1 == 9007199254740992)      "quotient - 1 lands on 2^53"
    assert (q1 /= 9007199254740992)          "quotient is not the rounded neighbour"

    let q2 = dz 4611686018427387905 3
    assert (show q2 == "1537228672809129301") "show of 2^62+1 div 3 is exact"
    -- Reconstruct the dividend: quotient * divisor + remainder.
    assert (q2 * 3 + mz 4611686018427387905 3 == 4611686018427387905)
        "dividend reconstructs from quotient and remainder"
    assert (q2 * 3 == 4611686018427387903)   "quotient * 3 exact"

    let q3 = dz 9223372036854775807 2
    assert (q3 == q2 * 3)                    "independent routes agree (2^62-1)"
    assert (show q3 == "4611686018427387903") "show of int64max div 2 is exact"

    -- Quotients flowing through a data-structure fold.
    assert (sumList [q1, 1, 1] == 9007199254740995) "fold over large quotient exact"

    -- A quotient used as a further dividend.
    assert (dz q1 3 == 3002399751580331)     "chained div stays exact"
    assert (mz q1 2 == 1)                    "chained mod sees the low bit"

    -- Ordering: a float-rounded q1 would collapse this strict chain.
    assert (9007199254740992 < q1 && q1 < 9007199254740994)
        "quotient sits strictly between its neighbours"

    putStrLn "ok"
