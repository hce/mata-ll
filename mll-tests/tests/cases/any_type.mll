-- Tests for the Any type (dynamic typing for Lua interop)

describeAny :: Any -> String
describeAny (AnyString s)  = "string:" ++ s
describeAny (AnyInteger n) = "integer:" ++ show n
describeAny (AnyNumber n)  = "number:" ++ show n
describeAny (AnyBool b)    = "bool:" ++ show b
describeAny AnyNull        = "null"

-- Extract integer or default
getInteger :: Any -> Integer -> Integer
getInteger (AnyInteger n) _ = n
getInteger _ def = def

-- Check if Any is null
isNull :: Any -> Bool
isNull AnyNull = True
isNull _ = False

-- Convert a list of Any to strings
describeAll :: [Any] -> [String]
describeAll [] = []
describeAll (x:xs) = describeAny x : describeAll xs

main :: IO ()
main = do
    -- Construction
    let s = AnyString "hello"
    let i = AnyInteger 42
    let n = AnyNumber 3.14
    let b = AnyBool True
    let nu = AnyNull

    -- Pattern matching / describe
    assert (describeAny s == "string:hello") "any string"
    assert (describeAny i == "integer:42") "any integer"
    assert (describeAny (AnyNumber 2.5) == "number:2.5") "any number"
    assert (describeAny b == "bool:True") "any bool"
    assert (describeAny nu == "null") "any null"

    -- Extraction with default
    assert (getInteger (AnyInteger 99) 0 == 99) "getInteger match"
    assert (getInteger (AnyString "x") 0 == 0) "getInteger default"
    assert (getInteger AnyNull (-1) == -1) "getInteger null default"

    -- Null check
    assert (isNull AnyNull) "isNull true"
    assert (not (isNull (AnyBool False))) "isNull false"
    assert (not (isNull (AnyInteger 0))) "isNull int false"

    -- List of Any values
    let vals = [AnyString "a", AnyInteger 1, AnyNull]
    assert (describeAll vals == ["string:a", "integer:1", "null"]) "describeAll"

    -- Any doesn't derive Eq (heterogeneous values),
    -- so equality is tested via pattern matching above.

    -- Nested in data structures
    let m = Just (AnyInteger 10)
    assert (getInteger (case m of { Just a -> a; Nothing -> AnyNull }) 0 == 10) "any in maybe"
