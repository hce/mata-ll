add :: Int -> Int -> Int
add a b = a + b

main :: IO ()
main = do
    -- Dollar operator
    assert ((id $ 42) == 42) "dollar operator"

    -- Backtick infix
    assert ((3 `add` 4) == 7) "backtick infix"

    -- Boolean operators
    assert (True && True) "and TT"
    assert (not (True && False)) "and TF"
    assert (True || False) "or TF"
    assert (not (False || False)) "or FF"

    -- Comparison
    assert (1 < 2) "lt"
    assert (2 > 1) "gt"
    assert (1 <= 1) "le"
    assert (1 >= 1) "ge"
    assert (not (1 == 2)) "neq"
