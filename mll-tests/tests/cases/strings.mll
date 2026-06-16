main :: IO ()
main = do
    assert ("Hello, " <> "World!" == "Hello, World!") "string concat"
    assert ("" <> "x" == "x") "empty concat left"
    assert ("x" <> "" == "x") "empty concat right"
    assert (show 42 == "42") "show integer"
    assert (show True == "True") "show bool true"
    assert (show False == "False") "show bool false"
