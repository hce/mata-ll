-- JSON codec tests: escape decoding, strict parsing, the serializer,
-- round-trips, and the ToJSON/FromJSON classes and combinators.
import JSON

-- A user-defined type with combinator-based codecs written as top-level
-- functions — the working hand-decode pattern today, and the same calls a
-- derived FromJSON will generate.
data Point = Point Integer Integer
    deriving (Eq)

pointToJSON :: Point -> Json
pointToJSON (Point x y) = JObj [("x", toJSONInteger x), ("y", toJSONInteger y)]

pointFromJSON :: Json -> Either String Point
pointFromJSON j = jContext "Point" (jBind (jFieldWith fromJSONInteger "x" j) (\x -> jBind (jFieldWith fromJSONInteger "y" j) (\y -> Right (Point x y))))

data Tagged = Tagged String (Maybe Integer)
    deriving (Eq)

taggedToJSON :: Tagged -> Json
taggedToJSON (Tagged name mn) = JObj [("name", toJSONString name), ("value", toJSONMaybe toJSONInteger mn)]

taggedFromJSON :: Json -> Either String Tagged
taggedFromJSON j = jContext "Tagged" (jBind (jFieldWith fromJSONString "name" j) (\name -> jBind (jOptFieldWith fromJSONInteger "value" j) (\mn -> Right (Tagged name mn))))

-- A user-defined type with hand-written ToJSON/FromJSON instances against
-- the imported classes (allowed because PtN is local to this module).
-- Instance bodies are self-contained: mata-ll type-checks instance methods
-- before it registers top-level function signatures, so an instance method
-- cannot call the codec combinators yet — a compiler limitation Phase 2
-- lifts before deriving can target the class.
data PtN = PtN Number Number
    deriving (Eq)

instance ToJSON PtN where
    toJSON (PtN x y) = JObj [("x", JNum x), ("y", JNum y)]

instance FromJSON PtN where
    fromJSON (JObj fields) = go fields Nothing Nothing
      where
        go [] (Just x) (Just y) = Right (PtN x y)
        go [] _ _ = Left "PtN: missing field"
        go (("x", JNum v) : rest) _ my = go rest (Just v) my
        go (("y", JNum v) : rest) mx _ = go rest mx (Just v)
        go (_ : rest) mx my = go rest mx my
    fromJSON _ = Left "PtN: expected an object"

-- Helpers -------------------------------------------------------------

mustParse :: String -> Json
mustParse s = case parseJSON s of
    Left _ -> JNull
    Right val -> val

-- The decoded text of a string literal, or a marker on failure.
parseS :: String -> String
parseS s = case parseJSON s of
    Right (JStr x) -> x
    Right _ -> "<NOT A STRING>"
    Left _ -> "<PARSE FAILED>"

isLeft :: Either String Json -> Bool
isLeft (Left _) = True
isLeft (Right _) = False

rightIntIs :: Either String Integer -> Integer -> Bool
rightIntIs (Right n) m = n == m
rightIntIs (Left _) _ = False

leftInt :: Either String Integer -> Bool
leftInt (Left _) = True
leftInt (Right _) = False

rightNumIs :: Either String Number -> Number -> Bool
rightNumIs (Right n) m = n == m
rightNumIs (Left _) _ = False

leftNum :: Either String Number -> Bool
leftNum (Left _) = True
leftNum (Right _) = False

rightStrIs :: Either String String -> String -> Bool
rightStrIs (Right a) b = a == b
rightStrIs (Left _) _ = False

leftStr :: Either String String -> Bool
leftStr (Left _) = True
leftStr (Right _) = False

rightBoolIs :: Either String Bool -> Bool -> Bool
rightBoolIs (Right a) b = a == b
rightBoolIs (Left _) _ = False

rightIntsIs :: Either String [Integer] -> [Integer] -> Bool
rightIntsIs (Right xs) ys = xs == ys
rightIntsIs (Left _) _ = False

leftInts :: Either String [Integer] -> Bool
leftInts (Left _) = True
leftInts (Right _) = False

rightMaybeIntIs :: Either String (Maybe Integer) -> Maybe Integer -> Bool
rightMaybeIntIs (Right a) b = a == b
rightMaybeIntIs (Left _) _ = False

rightJsonIs :: Either String Json -> Json -> Bool
rightJsonIs (Right a) b = a == b
rightJsonIs (Left _) _ = False

leftJson :: Either String Json -> Bool
leftJson (Left _) = True
leftJson (Right _) = False

rightPointIs :: Either String Point -> Point -> Bool
rightPointIs (Right a) b = a == b
rightPointIs (Left _) _ = False

leftPoint :: Either String Point -> Bool
leftPoint (Left _) = True
leftPoint (Right _) = False

rightTaggedIs :: Either String Tagged -> Tagged -> Bool
rightTaggedIs (Right a) b = a == b
rightTaggedIs (Left _) _ = False

-- parse . encode round-trip
rt :: Json -> Bool
rt v = case parseJSON (encodeJSON v) of
    Right v2 -> v2 == v
    Left _ -> False

-- encode . parse round-trip on already-canonical text
rtText :: String -> Bool
rtText s = case parseJSON s of
    Right v -> encodeJSON v == s
    Left _ -> False

getNumIs :: String -> Number -> Bool
getNumIs s expected = case parseJSON s of
    Right (JNum n) -> n == expected
    Right _ -> False
    Left _ -> False

bs :: String
bs = strChar 92

qq :: String
qq = strChar 34

main :: IO ()
main = do
    -- ============================================================
    -- Escape decoding
    -- ============================================================
    assert (parseS "\"a\\nb\"" == "a" <> strChar 10 <> "b") "decode \\n"
    assert (parseS "\"a\\tb\"" == "a" <> strChar 9 <> "b") "decode \\t"
    assert (parseS "\"a\\rb\"" == "a" <> strChar 13 <> "b") "decode \\r"
    assert (parseS "\"a\\bb\"" == "a" <> strChar 8 <> "b") "decode \\b"
    assert (parseS "\"a\\fb\"" == "a" <> strChar 12 <> "b") "decode \\f"
    assert (parseS "\"a\\\"b\"" == "a" <> qq <> "b") "decode escaped quote"
    assert (parseS "\"a\\\\b\"" == "a" <> bs <> "b") "decode escaped backslash"
    assert (parseS "\"a\\/b\"" == "a/b") "decode escaped slash"
    assert (parseS "\"\\\\n\"" == bs <> "n") "backslash-then-n stays two chars"
    assert (parseS "\"\\u0041\"" == "A") "\\u ASCII"
    assert (parseS "\"\\u00e9\"" == strChar 195 <> strChar 169) "\\u 2-byte UTF-8"
    assert (parseS "\"\\u20ac\"" == strChar 226 <> strChar 130 <> strChar 172) "\\u 3-byte UTF-8"
    assert (parseS "\"\\u20AC\"" == strChar 226 <> strChar 130 <> strChar 172) "\\u uppercase hex"
    assert (parseS "\"a\\u0000b\"" == "a" <> strChar 0 <> "b") "\\u0000 NUL"
    assert (parseS "\"\\ud83d\\ude00\"" == strChar 240 <> strChar 159 <> strChar 152 <> strChar 128) "surrogate pair U+1F600"
    assert (parseS "\"x\\uD834\\uDD1Ey\"" == "x" <> strChar 240 <> strChar 157 <> strChar 132 <> strChar 158 <> "y") "surrogate pair U+1D11E in context"

    -- Escape decoding errors
    assert (isLeft (parseJSON "\"\\x\"")) "invalid escape rejected"
    assert (isLeft (parseJSON "\"\\u12\"")) "truncated \\u rejected"
    assert (isLeft (parseJSON "\"\\u12g4\"")) "bad hex digit rejected"
    assert (isLeft (parseJSON "\"\\ud800\"")) "lone high surrogate rejected"
    assert (isLeft (parseJSON "\"\\ud800\\u0041\"")) "high surrogate without low rejected"
    assert (isLeft (parseJSON "\"\\udc00\"")) "lone low surrogate rejected"
    assert (isLeft (parseJSON ("\"a" <> strChar 10 <> "b\""))) "raw control char in string rejected"
    assert (isLeft (parseJSON "\"abc")) "unterminated string still rejected"

    -- ============================================================
    -- Trailing garbage rejection
    -- ============================================================
    assert (isLeft (parseJSON "1 x")) "trailing garbage after number"
    assert (isLeft (parseJSON "42 43")) "second value rejected"
    assert (isLeft (parseJSON "{} {}")) "second object rejected"
    assert (isLeft (parseJSON "null,")) "trailing comma rejected"
    assert (isLeft (parseJSON "[1,2] ]")) "trailing bracket rejected"
    assert (isLeft (parseJSON "\"a\" \"b\"")) "second string rejected"
    assert (not (isLeft (parseJSON "  42  "))) "surrounding whitespace ok"
    assert (not (isLeft (parseJSON " [1, 2] "))) "array with whitespace ok"

    -- ============================================================
    -- Strict number grammar
    -- ============================================================
    assert (isLeft (parseJSON "01")) "leading zero rejected"
    assert (isLeft (parseJSON "-")) "bare minus rejected"
    assert (isLeft (parseJSON "1e")) "empty exponent rejected"
    assert (isLeft (parseJSON "1e+")) "signed empty exponent rejected"
    assert (isLeft (parseJSON "1.")) "empty fraction rejected"
    assert (isLeft (parseJSON ".5")) "leading dot rejected"
    assert (isLeft (parseJSON "+1")) "leading plus rejected"
    assert (isLeft (parseJSON "1e2e3")) "double exponent rejected"
    assert (isLeft (parseJSON "1..2")) "double dot rejected"
    assert (getNumIs "-0" 0.0) "-0 ok"
    assert (getNumIs "0.5" 0.5) "0.5 ok"
    assert (getNumIs "1e+2" 100.0) "1e+2 ok"
    assert (getNumIs "1E2" 100.0) "capital E ok"
    assert (getNumIs "-12.5e-1" (-1.25)) "full grammar ok"
    assert (getNumIs "1230" 1230.0) "plain integer ok"

    -- ============================================================
    -- Serializer
    -- ============================================================
    assert (encodeJSON JNull == "null") "encode null"
    assert (encodeJSON (JBool True) == "true") "encode true"
    assert (encodeJSON (JBool False) == "false") "encode false"
    assert (encodeJSON (toJSONInteger 42) == "42") "encode integer"
    assert (encodeJSON (toJSONInteger (-7)) == "-7") "encode negative integer"
    assert (encodeJSON (JNum 3.5) == "3.5") "encode float"
    assert (encodeJSON (JStr "hi") == qq <> "hi" <> qq) "encode plain string"
    assert (encodeJSON (JStr "") == qq <> qq) "encode empty string"
    assert (encodeJSON (JStr ("a" <> strChar 10 <> "b")) == qq <> "a" <> bs <> "n" <> "b" <> qq) "escape newline"
    assert (encodeJSON (JStr ("a" <> strChar 9 <> "b")) == qq <> "a" <> bs <> "tb" <> qq) "escape tab"
    assert (encodeJSON (JStr ("a" <> qq <> "b")) == qq <> "a" <> bs <> qq <> "b" <> qq) "escape quote"
    assert (encodeJSON (JStr ("a" <> bs <> "b")) == qq <> "a" <> bs <> bs <> "b" <> qq) "escape backslash"
    assert (encodeJSON (JStr (strChar 1)) == qq <> bs <> "u0001" <> qq) "escape control as \\u0001"
    assert (encodeJSON (JStr (strChar 31)) == qq <> bs <> "u001f" <> qq) "escape control as \\u001f"
    assert (encodeJSON (JArr []) == "[]") "encode empty array"
    assert (encodeJSON (JObj []) == "{}") "encode empty object"
    assert (encodeJSON (JArr [toJSONInteger 1, JNull, JBool True]) == "[1,null,true]") "encode array"
    assert (encodeJSON (JObj [("a", JArr [toJSONInteger 1, JNull]), ("b", JStr "x")]) == "{" <> qq <> "a" <> qq <> ":[1,null]," <> qq <> "b" <> qq <> ":" <> qq <> "x" <> qq <> "}") "encode nested object"

    -- ============================================================
    -- Round-trips
    -- ============================================================
    assert (rt (JStr ("quote:" <> qq <> " back:" <> bs <> " nl:" <> strChar 10 <> " tab:" <> strChar 9))) "rt tricky string"
    assert (rt (JStr (strChar 226 <> strChar 130 <> strChar 172 <> " euro"))) "rt UTF-8 string"
    assert (rt (JStr (strChar 0 <> strChar 1 <> strChar 31))) "rt control chars"
    assert (rt (JObj [("k", JArr [toJSONInteger 1, JNum 2.5, JStr "s", JNull, JBool False]), ("m", JObj [])])) "rt nested"
    assert (rt (JNum 0.1)) "rt 0.1"
    assert (rt (JNum (toNumber "1e300"))) "rt 1e300"
    assert (rt (JNum (1.0 / 3.0))) "rt 1/3 exact"
    assert (rt (toJSONInteger 9007199254740993)) "rt integer beyond 2^53"
    assert (rt (toJSONInteger (-9223372036854775807))) "rt near Integer min"
    assert (rtText "[1,null,true,\"a\",{\"b\":2.5}]") "rt canonical text"
    assert (rtText "{\"x\":[[]],\"y\":{}}") "rt canonical nested text"

    -- decoded escapes re-encode to canonical form
    assert (encodeJSON (mustParse "\"a\\u0041\\n\"") == qq <> "aA" <> bs <> "n" <> qq) "escapes re-encode canonically"

    -- ============================================================
    -- Primitive codec combinators
    -- ============================================================
    assert (rightIntIs (fromJSONInteger (mustParse "42")) 42) "fromJSONInteger"
    assert (rightIntIs (fromJSONInteger (mustParse "-3")) (-3)) "fromJSONInteger negative"
    assert (rightIntIs (fromJSONInteger (mustParse "1e2")) 100) "fromJSONInteger integral float"
    assert (rightIntIs (fromJSONInteger (mustParse "2.0")) 2) "fromJSONInteger 2.0"
    assert (leftInt (fromJSONInteger (mustParse "3.5"))) "fromJSONInteger rejects non-integral"
    assert (leftInt (fromJSONInteger (mustParse "\"42\""))) "fromJSONInteger rejects string"
    assert (leftInt (fromJSONInteger (mustParse "1e300"))) "fromJSONInteger rejects out of range"
    assert (leftInt (fromJSONInteger JNull)) "fromJSONInteger rejects null"
    assert (rightNumIs (fromJSONNumber (mustParse "3.5")) 3.5) "fromJSONNumber"
    assert (leftNum (fromJSONNumber (mustParse "true"))) "fromJSONNumber rejects bool"
    assert (rightStrIs (fromJSONString (mustParse "\"hi\"")) "hi") "fromJSONString"
    assert (leftStr (fromJSONString (mustParse "5"))) "fromJSONString rejects number"
    assert (rightBoolIs (fromJSONBool (mustParse "true")) True) "fromJSONBool"
    assert (encodeJSON (toJSONNumber 2.5) == "2.5") "toJSONNumber"
    assert (encodeJSON (toJSONString "s") == qq <> "s" <> qq) "toJSONString"
    assert (encodeJSON (toJSONBool False) == "false") "toJSONBool"

    -- Lists
    assert (rightIntsIs (fromJSONList fromJSONInteger (mustParse "[1,2,3]")) [1, 2, 3]) "fromJSONList"
    assert (rightIntsIs (fromJSONList fromJSONInteger (mustParse "[]")) []) "fromJSONList empty"
    assert (leftInts (fromJSONList fromJSONInteger (mustParse "[1,\"x\"]"))) "fromJSONList bad element"
    assert (leftInts (fromJSONList fromJSONInteger (mustParse "{}"))) "fromJSONList rejects object"
    assert (encodeJSON (toJSONList toJSONInteger [1, 2]) == "[1,2]") "toJSONList"

    -- Maybe <-> null
    assert (rightMaybeIntIs (fromJSONMaybe fromJSONInteger (mustParse "null")) Nothing) "fromJSONMaybe null"
    assert (rightMaybeIntIs (fromJSONMaybe fromJSONInteger (mustParse "5")) (Just 5)) "fromJSONMaybe just"
    assert (encodeJSON (toJSONMaybe toJSONInteger Nothing) == "null") "toJSONMaybe nothing"
    assert (encodeJSON (toJSONMaybe toJSONInteger (Just 7)) == "7") "toJSONMaybe just"

    -- ============================================================
    -- Object decoder combinators
    -- ============================================================
    let obj = mustParse "{\"a\": 1, \"b\": null}"
    assert (rightJsonIs (jField "a" obj) (toJSONInteger 1)) "jField present"
    assert (leftJson (jField "zzz" obj)) "jField missing"
    assert (leftJson (jField "a" (mustParse "[1]"))) "jField on non-object"
    assert (rightIntIs (jFieldWith fromJSONInteger "a" obj) 1) "jFieldWith"
    assert (leftInt (jFieldWith fromJSONInteger "b" obj)) "jFieldWith wrong type"
    assert (rightMaybeIntIs (jOptFieldWith fromJSONInteger "a" obj) (Just 1)) "jOptFieldWith present"
    assert (rightMaybeIntIs (jOptFieldWith fromJSONInteger "zzz" obj) Nothing) "jOptFieldWith missing"
    assert (rightMaybeIntIs (jOptFieldWith fromJSONInteger "b" obj) Nothing) "jOptFieldWith null"
    case jExpectObj obj of
        Right fields -> assert (length fields == 2) "jExpectObj"
        Left _ -> assert False "jExpectObj"
    case jExpectArr (mustParse "[1,2]") of
        Right xs -> assert (length xs == 2) "jExpectArr"
        Left _ -> assert False "jExpectArr"
    case jExpectObj (mustParse "[1]") of
        Right _ -> assert False "jExpectObj rejects array"
        Left _ -> assert True "jExpectObj rejects array"

    -- ============================================================
    -- Hand-written codecs and class instances on user types
    -- ============================================================
    assert (encodeJSON (pointToJSON (Point 3 4)) == "{" <> qq <> "x" <> qq <> ":3," <> qq <> "y" <> qq <> ":4}") "combinator encode Point"
    assert (encodeJSON (toJSON (PtN 1.5 2.5)) == "{" <> qq <> "x" <> qq <> ":1.5," <> qq <> "y" <> qq <> ":2.5}") "toJSON dispatch on user type"
    assert (encodeToJSON (PtN 0.5 4.5) == "{" <> qq <> "x" <> qq <> ":0.5," <> qq <> "y" <> qq <> ":4.5}") "encodeToJSON constrained wrapper"
    assert (rightPointIs (decodeJSONWith pointFromJSON "{\"x\": 10, \"y\": 20}") (Point 10 20)) "decode Point"
    assert (leftPoint (decodeJSONWith pointFromJSON "{\"x\": 10}")) "decode Point missing field"
    assert (leftPoint (decodeJSONWith pointFromJSON "{\"x\": 10, \"y\": 1.5}")) "decode Point non-integral"
    assert (leftPoint (decodeJSONWith pointFromJSON "{\"x\": 1, \"y\": 2} trailing")) "decode Point trailing garbage"
    case decodeJSONWith pointFromJSON "[]" of
        Left e -> assert (e == "while decoding Point: expected an object with a field 'x', but found an array") "jContext error text"
        Right _ -> assert False "jContext error text"

    -- Maybe field through a full codec round-trip
    assert (rightTaggedIs (decodeJSONWith taggedFromJSON (encodeJSON (taggedToJSON (Tagged "a" (Just 5))))) (Tagged "a" (Just 5))) "Tagged rt just"
    assert (rightTaggedIs (decodeJSONWith taggedFromJSON (encodeJSON (taggedToJSON (Tagged "b" Nothing)))) (Tagged "b" Nothing)) "Tagged rt nothing"
