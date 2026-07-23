-- Regression guard (Finding 3, common case — APPLIES TO ALL TARGETS):
-- whatever the div-by-zero / large-integer fix looks like (e.g. Lua 5.4
-- `//` floor division, or an explicit-check helper), ordinary small
-- `div` and `mod` must keep their exact Haskell (floor-division)
-- results, including all four sign combinations:
--
--   GHC:  10 `div`   3  ==  3      10 `mod`   3  ==  1
--       (-7) `div`   2  == -4    (-7) `mod`   2  ==  1
--         7 `div` (-2)  == -4      7 `mod` (-2)  == -1
--       (-7) `div` (-2) ==  3    (-7) `mod` (-2) == -1
--
-- (Haskell `div` rounds toward negative infinity; `mod` takes the sign
-- of the divisor. Lua 5.4 `//` and `%` share exactly these semantics.)
--
-- The runtime path is exercised through opaque functions so constant
-- folding cannot mask it; the positive-operand literal forms cover the
-- folded path as well. Negative-divisor LITERAL forms live in the
-- separate probe file div_mod_negative_literal_folding.mll.

dz :: Int -> Int -> Int
dz a b = a `div` b

mz :: Int -> Int -> Int
mz a b = a `mod` b

-- The defining law: (a `div` b) * b + (a `mod` b) == a
law :: Int -> Int -> Bool
law a b = dz a b * b + mz a b == a

main :: IO ()
main = do
    -- Runtime path, all sign combinations (Haskell floor semantics).
    assert (dz 10 3 == 3)          "div ++ small"
    assert (mz 10 3 == 1)          "mod ++ small"
    assert (dz (-7) 2 == (-4))     "div -+ rounds toward -inf"
    assert (mz (-7) 2 == 1)        "mod -+ takes divisor sign"
    assert (dz 7 (-2) == (-4))     "div +- rounds toward -inf"
    assert (mz 7 (-2) == (-1))     "mod +- takes divisor sign"
    assert (dz (-7) (-2) == 3)     "div -- rounds toward -inf"
    assert (mz (-7) (-2) == (-1))  "mod -- takes divisor sign"

    -- Exact multiples and zero dividend.
    assert (dz 9 3 == 3)           "div exact multiple"
    assert (mz 9 3 == 0)           "mod exact multiple"
    assert (dz 0 5 == 0)           "zero dividend div"
    assert (mz 0 5 == 0)           "zero dividend mod"
    assert (dz 1 1 == 1)           "div by one"
    assert (mz 1 1 == 0)           "mod by one"

    -- The div/mod law over every sign combination.
    assert (law 10 3)              "law ++"
    assert (law (-7) 2)            "law -+"
    assert (law 7 (-2))            "law +-"
    assert (law (-7) (-2))         "law --"
    assert (law 0 3)               "law zero dividend"
    assert (law 12 4)              "law exact multiple"

    -- Folded path (positive literals; fold.rs computes these in i64).
    assert (10 `div` 3 == 3)       "literal div"
    assert (10 `mod` 3 == 1)       "literal mod"
    assert (100 `div` 10 == 10)    "literal div exact"
    assert (100 `mod` 10 == 0)     "literal mod exact"

    -- A small `div` result must still be a well-behaved integer in
    -- further arithmetic and show (guards against a fix accidentally
    -- producing floats, e.g. "3.0").
    let q = dz 10 3
    assert (q + 1 == 4)            "small quotient arithmetic"
    assert (show q == "3")         "small quotient shows as integer"
    assert (show (mz 10 3) == "1") "small remainder shows as integer"

    putStrLn "ok"
