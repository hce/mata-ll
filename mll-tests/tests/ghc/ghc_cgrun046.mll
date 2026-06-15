-- GHC cgrun046: Numeric edge cases
-- Tests integer arithmetic edge cases

main :: IO ()
main = do
    -- Large numbers
    assert (2 * 2 * 2 * 2 * 2 * 2 * 2 * 2 * 2 * 2 == 1024) "2^10"
    let big = 1000000000
    assert (big * big == 1000000000000000000) "10^18"

    -- Division and modulo
    assert (7 `div` 2 == 3) "div pos"
    assert ((-7) `div` 2 == -4) "div neg"
    assert (7 `mod` 2 == 1) "mod pos"
    assert ((-7) `mod` 2 == 1) "mod neg"

    -- Mixed arithmetic
    assert (3 + 4 * 5 == 23) "precedence"
    assert ((3 + 4) * 5 == 35) "parens"

    -- Negative numbers
    assert ((-1) * (-1) == 1) "neg * neg"
    assert (0 - 5 == -5) "sub to neg"

    -- abs via guards
    let abs_ x = if x < 0 then 0 - x else x
    assert (abs_ 5 == 5) "abs pos"
    assert (abs_ (-5) == 5) "abs neg"
    assert (abs_ 0 == 0) "abs zero"

    putStrLn "ok"
