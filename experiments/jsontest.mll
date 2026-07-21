import JSON

mustParse :: String -> Json
mustParse s = case parseJSON s of
    Left err -> JNull
    Right val -> val

getStr :: Json -> String
getStr v = case jString v of
    Just s -> s
    Nothing -> ""

getNum :: Json -> Number
getNum v = case jNumber v of
    Just n -> n
    Nothing -> 0.0

getBool :: Json -> Bool
getBool v = case jBool v of
    Just b -> b
    Nothing -> False

get :: String -> Json -> Json
get k v = case jLookup k v of
    Just x -> x
    Nothing -> JNull

idx :: Integer -> Json -> Json
idx i v = case jIndex i v of
    Just x -> x
    Nothing -> JNull

main :: IO ()
main = do
    assert (jIsNull (mustParse "null")) "null"
    assert (getBool (mustParse "true")) "true"
    assert (not (getBool (mustParse "false"))) "false"
    assert (getNum (mustParse "42") == 42.0) "integer"
    assert (getNum (mustParse "3.14") == 3.14) "float"
    assert (getNum (mustParse "-7") == -7.0) "negative"
    assert (getStr (mustParse "\"hello\"") == "hello") "string"
    assert (getStr (mustParse "\"\"") == "") "empty string"
    let arr = mustParse "[1, 2, 3]"
    assert (getNum (idx 0 arr) == 1.0) "array[0]"
    assert (getNum (idx 1 arr) == 2.0) "array[1]"
    assert (getNum (idx 2 arr) == 3.0) "array[2]"
    let obj = mustParse "{\"name\": \"Alice\", \"age\": 30}"
    assert (getStr (get "name" obj) == "Alice") "obj.name"
    assert (getNum (get "age" obj) == 30.0) "obj.age"
    assert (jIsNull (get "missing" obj)) "obj.missing"
    let nested = mustParse "{\"a\": [1, 2], \"b\": {\"c\": true}}"
    assert (getNum (idx 0 (get "a" nested)) == 1.0) "nested array"
    assert (getBool (get "c" (get "b" nested))) "nested object"
    let ws = mustParse "  { \"x\" : 1 }  "
    assert (getNum (get "x" ws) == 1.0) "whitespace"
    let mixed = mustParse "[1, \"two\", true, null]"
    assert (getNum (idx 0 mixed) == 1.0) "mixed num"
    assert (getStr (idx 1 mixed) == "two") "mixed str"
    assert (getBool (idx 2 mixed)) "mixed bool"
    assert (jIsNull (idx 3 mixed)) "mixed null"
    putStrLn "All JSON tests passed!"
