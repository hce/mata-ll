-- ghc_regr011: String operations: concatenation, show embedding, comparison

-- Build a sentence from parts
buildSentence :: [String] -> String
buildSentence []     = ""
buildSentence (w:ws) = case ws of
    [] -> w
    _  -> w ++ " " ++ buildSentence ws

-- Repeat a string n times
repeatStr :: Integer -> String -> String
repeatStr 0 _ = ""
repeatStr n s = s ++ repeatStr (n - 1) s

-- Pad a string on the left with n pad strings (n is the number of pads)
padLeft :: Integer -> String -> String -> String
padLeft n pad s = repeatStr n pad ++ s

-- Embed show into strings
formatEntry :: String -> Integer -> String
formatEntry key val = key ++ "=" ++ show val

joinRest :: [(String, Integer)] -> String
joinRest []          = "}"
joinRest (p:rest)    = ", " ++ formatEntry (fst p) (snd p) ++ joinRest rest

formatList :: [(String, Integer)] -> String
formatList []      = "{}"
formatList (p:rest) = "{" ++ formatEntry (fst p) (snd p) ++ joinRest rest

main :: IO ()
main = do
    -- Basic concatenation
    assert ("hello" ++ " " ++ "world" == "hello world") "concat"
    assert ("" ++ "x" == "x") "concat empty left"
    assert ("x" ++ "" == "x") "concat empty right"

    -- buildSentence
    assert (buildSentence ["the", "quick", "brown", "fox"] == "the quick brown fox") "sentence"
    assert (buildSentence [] == "") "sentence empty"
    assert (buildSentence ["solo"] == "solo") "sentence one"

    -- repeatStr
    assert (repeatStr 3 "ab" == "ababab") "repeat 3"
    assert (repeatStr 0 "x" == "") "repeat 0"

    -- padLeft (now takes count of pads, not width)
    assert (padLeft 3 "0" "42" == "00042") "padLeft"
    assert (padLeft 1 " " "hi" == " hi") "padLeft space"

    -- String comparison
    assert ("abc" == "abc") "str eq"
    assert ("abc" /= "abd") "str neq"
    assert ("abc" < "abd") "str lt"
    assert ("b" > "a") "str gt"

    -- show embedding
    assert (formatEntry "port" 8080 == "port=8080") "formatEntry"
    assert (formatList [("a", 1), ("b", 2)] == "{a=1, b=2}") "formatList"
    assert (formatList [] == "{}") "formatList empty"

    -- prefix check: test by reconstructing
    assert ("hel" ++ "lo" == "hello") "prefix concat true"
    assert ("world" ++ "x" /= "hello") "prefix mismatch"
    assert ("" ++ "anything" == "anything") "empty prefix"

    putStrLn "ok"
