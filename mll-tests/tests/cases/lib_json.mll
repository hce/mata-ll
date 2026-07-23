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

idx :: Int -> Json -> Json
idx i v = case jIndex i v of
    Just x -> x
    Nothing -> JNull

isLeft :: Either String Json -> Bool
isLeft (Left _) = True
isLeft (Right _) = False

main :: IO ()
main = do
    -- Primitives
    assert (jIsNull (mustParse "null")) "parse null"
    assert (getBool (mustParse "true")) "parse true"
    assert (not (getBool (mustParse "false"))) "parse false"

    -- Numbers
    assert (getNum (mustParse "42") == 42.0) "parse integer"
    assert (getNum (mustParse "3.14") == 3.14) "parse float"
    assert (getNum (mustParse "-7") == -7.0) "parse negative"
    assert (getNum (mustParse "0") == 0.0) "parse zero"
    assert (getNum (mustParse "1e2") == 100.0) "parse scientific"
    assert (getNum (mustParse "-0.5") == -0.5) "parse neg float"

    -- Strings
    assert (getStr (mustParse "\"hello\"") == "hello") "parse string"
    assert (getStr (mustParse "\"\"") == "") "parse empty string"
    assert (getStr (mustParse "\"a b c\"") == "a b c") "parse string with spaces"

    -- Arrays
    let arr = mustParse "[1, 2, 3]"
    assert (getNum (idx 0 arr) == 1.0) "array[0]"
    assert (getNum (idx 1 arr) == 2.0) "array[1]"
    assert (getNum (idx 2 arr) == 3.0) "array[2]"
    assert (jIsNull (idx 3 arr)) "array out of bounds"

    -- Empty array
    let empty = mustParse "[]"
    assert (jIsNull (idx 0 empty)) "empty array access"

    -- Nested arrays
    let nested = mustParse "[[1, 2], [3, 4]]"
    assert (getNum (idx 0 (idx 0 nested)) == 1.0) "nested array [0][0]"
    assert (getNum (idx 1 (idx 1 nested)) == 4.0) "nested array [1][1]"

    -- Objects
    let obj = mustParse "{\"name\": \"Alice\", \"age\": 30}"
    assert (getStr (get "name" obj) == "Alice") "obj.name"
    assert (getNum (get "age" obj) == 30.0) "obj.age"
    assert (jIsNull (get "missing" obj)) "obj missing key"

    -- Empty object
    let emptyObj = mustParse "{}"
    assert (jIsNull (get "x" emptyObj)) "empty obj access"

    -- Nested objects
    let deep = mustParse "{\"a\": {\"b\": {\"c\": 42}}}"
    assert (getNum (get "c" (get "b" (get "a" deep))) == 42.0) "deep nested obj"

    -- Mixed types in array
    let mixed = mustParse "[1, \"two\", true, null, false]"
    assert (getNum (idx 0 mixed) == 1.0) "mixed num"
    assert (getStr (idx 1 mixed) == "two") "mixed str"
    assert (getBool (idx 2 mixed)) "mixed true"
    assert (jIsNull (idx 3 mixed)) "mixed null"
    assert (not (getBool (idx 4 mixed))) "mixed false"

    -- Whitespace handling
    assert (getNum (get "x" (mustParse "  { \"x\" : 1 }  ")) == 1.0) "whitespace around obj"
    assert (getNum (idx 0 (mustParse " [ 42 ] ")) == 42.0) "whitespace around array"

    -- Object with array value
    let combo = mustParse "{\"items\": [10, 20, 30]}"
    assert (getNum (idx 1 (get "items" combo)) == 20.0) "obj with array value"

    -- Error cases
    assert (isLeft (parseJSON "")) "error: empty input"
    assert (isLeft (parseJSON "nul")) "error: truncated null"
    assert (isLeft (parseJSON "{")) "error: unclosed object"
    assert (isLeft (parseJSON "[")) "error: unclosed array"
    assert (isLeft (parseJSON "\"unterminated")) "error: unclosed string"

    -- Accessor type mismatches
    assert (jIsNull (get "key" (mustParse "42"))) "lookup on non-object"
    assert (jIsNull (idx 0 (mustParse "\"str\""))) "index on non-array"
