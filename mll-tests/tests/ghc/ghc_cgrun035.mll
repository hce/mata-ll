-- GHC cgrun035: String operations
-- Tests string manipulation (String = Lua string)

main :: IO ()
main = do
    -- Concatenation
    assert ("hello" <> " " <> "world" == "hello world") "concat"
    assert ("" <> "x" == "x") "concat empty left"
    assert ("x" <> "" == "x") "concat empty right"

    -- Show embedding
    assert ("value: " <> show 42 == "value: 42") "show embed"
    assert ("flag: " <> show True == "flag: True") "show bool embed"

    -- String comparison
    assert ("abc" == "abc") "eq"
    assert ("abc" /= "def") "neq"
    assert ("a" < "b") "lt"
    assert ("b" > "a") "gt"
    assert ("abc" < "abd") "lt prefix"
    assert ("" < "a") "empty lt"

    -- Multi-line building
    let msg = "line1\nline2\nline3"
    assert (msg == "line1\nline2\nline3") "multiline"

    putStrLn "ok"
