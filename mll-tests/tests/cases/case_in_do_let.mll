-- Regression test: multi-line case in do-let must not consume
-- the next do-statement as an argument.

classify :: Int -> String
classify n = case n of
    0 -> "zero"
    1 -> "one"
    _ -> "other"

main :: IO ()
main = do
    -- Case expression in do-let: next statement must not be swallowed
    let label = case 1 of
            0 -> "zero"
            1 -> "one"
            _ -> "other"
    assert (label == "one") "case in do-let"

    -- Case expression in do-let followed by another let
    let x = case 2 of
            0 -> "zero"
            _ -> "other"
    let y = "hello"
    assert (x == "other") "case in do-let value"
    assert (y == "hello") "let after case-let"

    -- Case result used in a bind
    let z = case 0 of
            0 -> "zero"
            _ -> "nope"
    putStrLn z
    assert (z == "zero") "case result bind"

    pure ()
