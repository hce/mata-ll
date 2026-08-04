import LString (strByte, strLen, strSub, strChar)
import Data.Generics

-- JSON value type
data Json = JNull | JBool Bool | JNum Number | JStr String | JArr [Json] | JObj [(String, Json)]
    deriving (Eq)

-- Parse result
data JResult = JOk Json Int | JErr String

-- Internal result for strings (avoids wrapping in Json)
data SResult = SOk String Int | SErr String

-- Internal result for arrays
data AResult = AOk [Json] Int | AErr String

-- Internal result for object pairs
data OResult = OOk [(String, Json)] Int | OErr String

-- ================================================================
-- Public API
-- ================================================================

parseJSON :: String -> Either String Json
parseJSON s = parseTop s (strLen s)

parseTop :: String -> Int -> Either String Json
parseTop s len = case skipWS s 1 len of
    pos -> case parseValue s pos len of
        JErr e -> Left e
        JOk val pos2 -> if pos2 > len
            then Right val
            else Left ("Unexpected character '" <> strSub s pos2 pos2 <> "' at position " <> show pos2 <> ": the input continues after the end of the JSON value")

-- Serialize a Json value to compact JSON text (no whitespace). This is the
-- inverse of parseJSON: parseJSON (encodeJSON v) == Right v for every value
-- except non-finite numbers (NaN and infinity have no JSON representation and
-- are emitted as null, matching JavaScript's JSON.stringify).
encodeJSON :: Json -> String
encodeJSON JNull = "null"
encodeJSON (JBool True) = "true"
encodeJSON (JBool False) = "false"
encodeJSON (JNum n) = encodeNum n
encodeJSON (JStr s) = encodeStr s
encodeJSON (JArr xs) = "[" <> encodeElems xs <> "]"
encodeJSON (JObj fields) = "{" <> encodePairs fields <> "}"

-- Serialize any ToJSON value straight to JSON text.
encodeToJSON :: ToJSON a => a -> String
encodeToJSON x = encodeJSON (toJSON x)

-- Parse JSON text and decode it with fromJSON. When the surrounding context
-- does not pin the result type, ascribe it at the call site — exactly like
-- `read`: `decodeJSON s :: Either String Person`.
decodeJSON :: FromJSON a => String -> Either String a
decodeJSON s = case parseJSON s of
    Left e -> Left e
    Right j -> fromJSON j

-- Parse JSON text and decode it with an explicit decoder function.
decodeJSONWith :: (Json -> Either String a) -> String -> Either String a
decodeJSONWith dec s = case parseJSON s of
    Left e -> Left e
    Right j -> dec j

-- ================================================================
-- Typeclasses
--
-- The primitive codecs come in two forms: the toJSON*/fromJSON* combinators
-- below, which the native `deriving (ToJSON/FromJSON)` codecs call
-- directly, and the `instance ToJSON Int`-style declarations further down
-- that wrap them, which the generic codecs' leaves resolve against. (Orphan
-- checking is relaxed for library modules, so this stdlib module may carry
-- instances for builtin types.) Write instances for your own data types in
-- terms of the combinators, or `deriving (Generic)` and use
-- genericToJSON/genericFromJSON.
-- ================================================================

class ToJSON a where
    toJSON :: a -> Json

class FromJSON a where
    fromJSON :: Json -> Either String a
    -- How a value of this type is read out of one field of a JSON object:
    -- required by default. The Maybe instance overrides it so a missing key
    -- and an explicit null both decode to Nothing — the same optionality the
    -- derived decoders give Maybe record fields.
    fromJSONField :: String -> Json -> Either String a
    fromJSONField k j = jFieldWith fromJSON k j

-- ================================================================
-- Primitive codecs (combinators)
-- ================================================================

-- A short description of a value's JSON type, for error messages.
jTypeName :: Json -> String
jTypeName JNull = "null"
jTypeName (JBool _) = "a boolean"
jTypeName (JNum _) = "a number"
jTypeName (JStr _) = "a string"
jTypeName (JArr _) = "an array"
jTypeName (JObj _) = "an object"

toJSONInt :: Int -> Json
toJSONInt n = JNum (intToNum n)

-- Decode a number that must be integral. A non-integral number (3.5) and a
-- number outside the 64-bit Int range are both rejected with a clear error.
fromJSONInt :: Json -> Either String Int
fromJSONInt (JNum n) = numToInteger n
fromJSONInt j = Left ("expected an integer, but found " <> jTypeName j)

-- A number with Lua's integer subtype is exact and in range by construction
-- (the parser only produces one for in-range integer syntax). Floats go
-- through the integrality and range checks.
numToInteger :: Number -> Either String Int
numToInteger n = if numMathType n == "integer"
    then Right (numFloor n)
    else numToIntegerFloat n

-- The float bounds are strict on BOTH sides: 2^63 and -2^63 are exactly
-- representable as doubles, and any float equal to one of them may have
-- rounded there from an out-of-range neighbour (e.g. -9223372036854775809
-- becomes exactly -2^63), so accepting the boundary float would silently
-- decode a wrong value. The true minimum -9223372036854775808 still decodes
-- exactly — the parser special-cases its integer spelling (see numExact).
-- note: the one casualty is float syntax for the exact minimum, like
-- -9223372036854775808.0, which is rejected as out of range; GHC's aeson
-- accepts it because it parses numbers exactly (Scientific), which a Lua
-- double cannot reproduce.
numToIntegerFloat :: Number -> Either String Int
numToIntegerFloat n = case numModf n of
    (_, frac) -> if frac /= 0.0
        then Left ("expected an integer, but found the non-integral number " <> encodeNum n)
        else if n > -9223372036854775808.0 && n < 9223372036854775808.0
            then Right (numFloor n)
            else Left ("the number " <> encodeNum n <> " is outside the 64-bit Int range")

toJSONNumber :: Number -> Json
toJSONNumber n = JNum n

fromJSONNumber :: Json -> Either String Number
fromJSONNumber (JNum n) = Right n
fromJSONNumber j = Left ("expected a number, but found " <> jTypeName j)

toJSONString :: String -> Json
toJSONString s = JStr s

fromJSONString :: Json -> Either String String
fromJSONString (JStr s) = Right s
fromJSONString j = Left ("expected a string, but found " <> jTypeName j)

toJSONBool :: Bool -> Json
toJSONBool b = JBool b

fromJSONBool :: Json -> Either String Bool
fromJSONBool (JBool b) = Right b
fromJSONBool j = Left ("expected a boolean, but found " <> jTypeName j)

toJSONList :: (a -> Json) -> [a] -> Json
toJSONList enc xs = JArr (map enc xs)

fromJSONList :: (Json -> Either String a) -> Json -> Either String [a]
fromJSONList dec (JArr xs) = decodeElems dec xs 0
fromJSONList _ j = Left ("expected an array, but found " <> jTypeName j)

decodeElems :: (Json -> Either String a) -> [Json] -> Int -> Either String [a]
decodeElems _ [] _ = Right []
decodeElems dec (x:xs) i = case dec x of
    Left e -> Left ("at array index " <> show i <> ": " <> e)
    Right v -> case decodeElems dec xs (i + 1) of
        Left e -> Left e
        Right vs -> Right (v : vs)

-- Nothing <-> null
toJSONMaybe :: (a -> Json) -> Maybe a -> Json
toJSONMaybe _ Nothing = JNull
toJSONMaybe enc (Just x) = enc x

fromJSONMaybe :: (Json -> Either String a) -> Json -> Either String (Maybe a)
fromJSONMaybe _ JNull = Right Nothing
fromJSONMaybe dec j = case dec j of
    Left e -> Left e
    Right v -> Right (Just v)

-- ================================================================
-- Decoder combinators for objects
-- ================================================================

jExpectObj :: Json -> Either String [(String, Json)]
jExpectObj (JObj fields) = Right fields
jExpectObj j = Left ("expected an object, but found " <> jTypeName j)

jExpectArr :: Json -> Either String [Json]
jExpectArr (JArr xs) = Right xs
jExpectArr j = Left ("expected an array, but found " <> jTypeName j)

-- Look up a required field in an object.
jField :: String -> Json -> Either String Json
jField k (JObj fields) = jFieldLookup k fields
jField k j = Left ("expected an object with a field '" <> k <> "', but found " <> jTypeName j)

jFieldLookup :: String -> [(String, Json)] -> Either String Json
jFieldLookup k [] = Left ("the required field '" <> k <> "' is missing")
jFieldLookup k ((fk, fv) : rest) = if k == fk then Right fv else jFieldLookup k rest

-- Look up a required field and decode it, tagging errors with the field name.
jFieldWith :: (Json -> Either String a) -> String -> Json -> Either String a
jFieldWith dec k j = case jField k j of
    Left e -> Left e
    Right v -> case dec v of
        Left e -> Left ("in field '" <> k <> "': " <> e)
        Right x -> Right x

-- Look up an optional field: a missing field and an explicit null both
-- decode to Nothing. Still an error when the value is not an object at all.
jOptFieldWith :: (Json -> Either String a) -> String -> Json -> Either String (Maybe a)
jOptFieldWith dec k j = case jExpectObj j of
    Left e -> Left e
    Right fields -> case jFieldLookup k fields of
        Left _ -> Right Nothing
        Right JNull -> Right Nothing
        Right v -> case dec v of
            Left e -> Left ("in field '" <> k <> "': " <> e)
            Right x -> Right (Just x)

-- Sequence decode steps: monadic bind specialized to Either String.
jBind :: Either String a -> (a -> Either String b) -> Either String b
jBind (Left e) _ = Left e
jBind (Right x) f = f x

-- Tag a decoder's errors with the name of the type being decoded.
jContext :: String -> Either String a -> Either String a
jContext _ (Right x) = Right x
jContext ctx (Left e) = Left ("while decoding " <> ctx <> ": " <> e)

-- ================================================================
-- Combinators for derived ToJSON encoders and FromJSON decoders
--
-- `deriving (ToJSON)` / `deriving (FromJSON)` generate a codec over these.
-- The convention (mirroring aeson's defaultOptions where mata-ll can):
--   * a single-constructor record maps to an object keyed by the field
--     names:                  data P = P { x :: Int }   ⇔  {"x":1}
--   * a single positional constructor maps to its argument itself
--     (one field) or an array of its arguments (several fields):
--                             data W = W Int            ⇔  7
--                             data V = V Int String     ⇔  [7,"a"]
--   * a multi-constructor type is tagged: either the bare constructor
--     name as a string (nullary constructors only), or an object with a
--     "tag" field — record fields inline in the same object, positional
--     arguments under "contents":
--                             data S = A | B Int | C { n :: Int }
--                               ⇔  "A"  or  {"tag":"A"}
--                               ⇔  {"tag":"B","contents":7}
--                               ⇔  {"tag":"C","n":3}
--   * a Maybe field encodes Nothing as null; it decodes from a missing
--     key, null, or the value itself.
--   * a field renamed with `as "key"` uses "key" as its JSON object key
--     in both directions (the same shared external name LuaDict uses as
--     the Lua table key).
--   * a constructor renamed with `Con field-types as "name"` uses "name"
--     as its tag in both directions — the bare string of a nullary
--     constructor and the "tag" value of a fielded one. Show and the
--     runtime representation keep the source constructor name; the
--     rename exists only at the JSON boundary.
--   * unknown object keys are ignored on decode, as aeson does.
--
-- note: aeson encodes only ALL-nullary sum types as bare strings; the
-- derived encoder emits the bare string for every nullary constructor
-- (even in a mixed sum, where aeson would emit {"tag":"A"}), and the
-- derived decoder accepts both forms, so the pair round-trips and both
-- aeson spellings decode.
-- note: Maybe (Maybe a) cannot round-trip under the null-is-Nothing
-- convention — Just Nothing has no JSON form distinct from Nothing.
-- ================================================================

-- Identity encoder: emit a raw Json field as-is.
toJSONValue :: Json -> Json
toJSONValue j = j

-- Identity decoder: keep the raw Json value (for fields of type Json).
fromJSONValue :: Json -> Either String Json
fromJSONValue j = Right j

-- The n-th element (0-based) of a list of Json values. Total: out of range
-- yields JNull, but derived decoders only index after jExpectArrN has
-- checked the arity.
jNth :: Int -> [Json] -> Json
jNth _ [] = JNull
jNth 0 (x:_) = x
jNth n (_:xs) = jNth (n - 1) xs

-- Expect the positional arguments of constructor `con` as an array of
-- exactly n elements.
jExpectArrN :: String -> Int -> Json -> Either String [Json]
jExpectArrN con n j = case jExpectArr j of
    Left e -> Left ("in the arguments of constructor '" <> con <> "': " <> e)
    Right xs -> if length xs == n
        then Right xs
        else Left ("constructor '" <> con <> "' takes " <> show n <> " arguments, but the array has " <> show (length xs))

-- Decode argument #i (1-based, for messages) of constructor `con`.
jArgWith :: (Json -> Either String a) -> String -> Int -> Json -> Either String a
jArgWith dec con i j = case dec j of
    Left e -> Left ("in argument " <> show i <> " of constructor '" <> con <> "': " <> e)
    Right x -> Right x

-- The value cannot start a tagged-constructor decode at all.
jExpectTagged :: Json -> Either String a
jExpectTagged j = Left ("expected an object with a \"tag\" field naming the constructor (or a bare constructor-name string), but found " <> jTypeName j)

-- A tag that names no constructor of the type being decoded.
jBadTag :: String -> String -> Either String a
jBadTag expected tag = Left ("the tag '" <> tag <> "' names no constructor of this type: expected " <> expected)

-- A bare string named a constructor that has fields.
jTagNeedsObject :: String -> Either String a
jTagNeedsObject con = Left ("the constructor '" <> con <> "' has fields, so a bare string cannot encode it: expected an object with \"tag\":\"" <> con <> "\"")

-- ================================================================
-- Value parser
-- ================================================================

parseValue :: String -> Int -> Int -> JResult
parseValue s pos len = if pos > len then JErr "Unexpected end of input" else dispatchValue s pos len (strByte s pos)

dispatchValue :: String -> Int -> Int -> Int -> JResult
dispatchValue s pos len 110 = parseNull s pos len
dispatchValue s pos len 116 = parseTrue s pos len
dispatchValue s pos len 102 = parseFalse s pos len
dispatchValue s pos len 34 = parseStringVal s pos len
dispatchValue s pos len 91 = parseArray s pos len
dispatchValue s pos len 123 = parseObject s pos len
dispatchValue s pos len 45 = parseNumber s pos len
dispatchValue s pos len c = if c >= 48 && c <= 57 then parseNumber s pos len else JErr ("Unexpected character at position " <> show pos)

-- ================================================================
-- Null, true, false
-- ================================================================

parseNull :: String -> Int -> Int -> JResult
parseNull s pos len = if pos + 3 <= len && strSub s pos (pos + 3) == "null" then JOk JNull (skipWS s (pos + 4) len) else JErr "Expected 'null'"

parseTrue :: String -> Int -> Int -> JResult
parseTrue s pos len = if pos + 3 <= len && strSub s pos (pos + 3) == "true" then JOk (JBool True) (skipWS s (pos + 4) len) else JErr "Expected 'true'"

parseFalse :: String -> Int -> Int -> JResult
parseFalse s pos len = if pos + 4 <= len && strSub s pos (pos + 4) == "false" then JOk (JBool False) (skipWS s (pos + 5) len) else JErr "Expected 'false'"

-- ================================================================
-- Numbers
--
-- Follows the JSON grammar exactly: -?int frac? exp?, where int has no
-- leading zeros, and frac/exp require at least one digit. Enforcing the
-- grammar here guarantees the matched text is always a valid Lua numeral,
-- so toNumber can never return garbage. Int-syntax numbers parse to
-- Lua's 64-bit integer subtype (exact for the full Int range) and
-- float syntax parses to a double, so no information is lost that a later
-- integrality check (fromJSONInt) would need.
-- ================================================================

parseNumber :: String -> Int -> Int -> JResult
parseNumber s pos len = if strByte s pos == 45 then numIntStart s (pos + 1) len pos else numIntStart s pos len pos

numIntStart :: String -> Int -> Int -> Int -> JResult
numIntStart s pos len start = if pos > len then JErr ("Invalid number at position " <> show start <> ": expected a digit") else numIntByte s pos len start (strByte s pos)

numIntByte :: String -> Int -> Int -> Int -> Int -> JResult
numIntByte s pos len start 48 = numAfterInt s (pos + 1) len start
numIntByte s pos len start c = if c >= 49 && c <= 57 then numDigits s (pos + 1) len start else JErr ("Invalid number at position " <> show start <> ": expected a digit")

numDigits :: String -> Int -> Int -> Int -> JResult
numDigits s pos len start = if pos <= len && isDigitByte (strByte s pos) then numDigits s (pos + 1) len start else numAfterInt s pos len start

numAfterInt :: String -> Int -> Int -> Int -> JResult
numAfterInt s pos len start = if pos > len then numFinish s start pos else numAfterIntByte s pos len start (strByte s pos)

numAfterIntByte :: String -> Int -> Int -> Int -> Int -> JResult
numAfterIntByte s pos len start 46 = numFracStart s (pos + 1) len start
numAfterIntByte s pos len start 101 = numExpSign s (pos + 1) len start
numAfterIntByte s pos len start 69 = numExpSign s (pos + 1) len start
numAfterIntByte s pos len start c = if isDigitByte c then JErr ("Invalid number at position " <> show start <> ": JSON does not allow leading zeros") else numFinish s start pos

numFracStart :: String -> Int -> Int -> Int -> JResult
numFracStart s pos len start = if pos <= len && isDigitByte (strByte s pos) then numFracDigits s (pos + 1) len start else JErr ("Invalid number at position " <> show start <> ": at least one digit is required after the decimal point")

numFracDigits :: String -> Int -> Int -> Int -> JResult
numFracDigits s pos len start = if pos <= len && isDigitByte (strByte s pos) then numFracDigits s (pos + 1) len start else numAfterFrac s pos len start

numAfterFrac :: String -> Int -> Int -> Int -> JResult
numAfterFrac s pos len start = if pos <= len && (strByte s pos == 101 || strByte s pos == 69) then numExpSign s (pos + 1) len start else numFinish s start pos

numExpSign :: String -> Int -> Int -> Int -> JResult
numExpSign s pos len start = if pos <= len && (strByte s pos == 43 || strByte s pos == 45) then numExpStart s (pos + 1) len start else numExpStart s pos len start

numExpStart :: String -> Int -> Int -> Int -> JResult
numExpStart s pos len start = if pos <= len && isDigitByte (strByte s pos) then numExpDigits s (pos + 1) len start else JErr ("Invalid number at position " <> show start <> ": at least one digit is required in the exponent")

numExpDigits :: String -> Int -> Int -> Int -> JResult
numExpDigits s pos len start = if pos <= len && isDigitByte (strByte s pos) then numExpDigits s (pos + 1) len start else numFinish s start pos

numFinish :: String -> Int -> Int -> JResult
numFinish s start pos = JOk (JNum (numExact (strSub s start (pos - 1)))) (skipWS s pos (strLen s))

-- Convert matched number text to a Lua number, with one special case: the
-- exact spelling of the minimum 64-bit integer. Lua's tonumber reads it as
-- -(9223372036854775808); the positive magnitude overflows the integer
-- subtype, so the result comes back as a float — indistinguishable from
-- out-of-range neighbours like -9223372036854775809 that round onto the
-- same double. Converting this one spelling by hand keeps the entire int64
-- range decodable with integer precision.
numExact :: String -> Number
numExact t = if t == "-9223372036854775808"
    then intToNum (0 - 9223372036854775807 - 1)
    else toNumber t

isDigitByte :: Int -> Bool
isDigitByte c = c >= 48 && c <= 57

toNumber :: String -> LuaPure "tonumber" Number

-- ================================================================
-- Strings
--
-- The scanner copies raw runs of bytes in whole chunks (chunkStart..pos-1)
-- and decodes every escape sequence to the bytes it denotes: the standard
-- one-character escapes, \uXXXX BMP code points (emitted as UTF-8), and
-- surrogate pairs for code points above U+FFFF. Lone surrogates and raw
-- control characters are rejected, as the JSON grammar requires.
-- ================================================================

parseStringVal :: String -> Int -> Int -> JResult
parseStringVal s pos len = case parseStr s (pos + 1) len of
    SErr e -> JErr e
    SOk str pos2 -> JOk (JStr str) pos2

parseStr :: String -> Int -> Int -> SResult
parseStr s pos len = scanStr s len pos pos ""

scanStr :: String -> Int -> Int -> Int -> String -> SResult
scanStr s len chunkStart pos acc = if pos > len then SErr "Unterminated string" else scanStrByte s len chunkStart pos acc (strByte s pos)

scanStrByte :: String -> Int -> Int -> Int -> String -> Int -> SResult
scanStrByte s len chunkStart pos acc 34 = SOk (acc <> strSub s chunkStart (pos - 1)) (skipWS s (pos + 1) len)
scanStrByte s len chunkStart pos acc 92 = scanEsc s len (pos + 1) (acc <> strSub s chunkStart (pos - 1))
scanStrByte s len chunkStart pos acc c = if c < 32 then SErr ("Control character (byte " <> show c <> ") at position " <> show pos <> ": control characters must be escaped inside a JSON string") else scanStr s len chunkStart (pos + 1) acc

-- pos is the character after the backslash.
scanEsc :: String -> Int -> Int -> String -> SResult
scanEsc s len pos acc = if pos > len then SErr "Unterminated escape sequence" else scanEscByte s len pos acc (strByte s pos)

scanEscByte :: String -> Int -> Int -> String -> Int -> SResult
scanEscByte s len pos acc 34 = scanStr s len (pos + 1) (pos + 1) (acc <> strChar 34)
scanEscByte s len pos acc 92 = scanStr s len (pos + 1) (pos + 1) (acc <> strChar 92)
scanEscByte s len pos acc 47 = scanStr s len (pos + 1) (pos + 1) (acc <> strChar 47)
scanEscByte s len pos acc 98 = scanStr s len (pos + 1) (pos + 1) (acc <> strChar 8)
scanEscByte s len pos acc 102 = scanStr s len (pos + 1) (pos + 1) (acc <> strChar 12)
scanEscByte s len pos acc 110 = scanStr s len (pos + 1) (pos + 1) (acc <> strChar 10)
scanEscByte s len pos acc 114 = scanStr s len (pos + 1) (pos + 1) (acc <> strChar 13)
scanEscByte s len pos acc 116 = scanStr s len (pos + 1) (pos + 1) (acc <> strChar 9)
scanEscByte s len pos acc 117 = scanU s len pos acc
scanEscByte s len pos acc _ = SErr ("Invalid escape sequence '\\" <> strSub s pos pos <> "' at position " <> show pos)

-- pos is at the 'u'; the four hex digits are pos+1..pos+4.
scanU :: String -> Int -> Int -> String -> SResult
scanU s len pos acc = if pos + 4 > len then SErr ("Truncated \\u escape at position " <> show pos <> ": four hex digits are required") else scanUCode s len pos acc (readHex4 s (pos + 1))

scanUCode :: String -> Int -> Int -> String -> Int -> SResult
scanUCode s len pos acc cp = if cp < 0
    then SErr ("Invalid \\u escape at position " <> show pos <> ": four hex digits are required")
    else if cp >= 55296 && cp <= 56319
        then scanUPair s len pos acc cp
        else if cp >= 56320 && cp <= 57343
            then SErr ("Lone low surrogate \\u escape at position " <> show pos <> ": a low surrogate (\\uDC00-\\uDFFF) is only valid immediately after a high surrogate")
            else scanStr s len (pos + 5) (pos + 5) (acc <> utf8Encode cp)

-- A high surrogate must be followed by \uXXXX with a low surrogate; together
-- they encode one code point above U+FFFF. The low escape spans pos+5..pos+10.
scanUPair :: String -> Int -> Int -> String -> Int -> SResult
scanUPair s len pos acc hi = if pos + 10 > len || strByte s (pos + 5) /= 92 || strByte s (pos + 6) /= 117
    then loneHighErr pos
    else scanUPairLow s len pos acc hi (readHex4 s (pos + 7))

scanUPairLow :: String -> Int -> Int -> String -> Int -> Int -> SResult
scanUPairLow s len pos acc hi lo = if lo >= 56320 && lo <= 57343
    then scanStr s len (pos + 11) (pos + 11) (acc <> utf8Encode (65536 + (hi - 55296) * 1024 + (lo - 56320)))
    else loneHighErr pos

loneHighErr :: Int -> SResult
loneHighErr pos = SErr ("Unpaired high surrogate \\u escape at position " <> show pos <> ": JSON encodes characters above U+FFFF as a surrogate pair, so a high surrogate (\\uD800-\\uDBFF) must be followed immediately by a low surrogate (\\uDC00-\\uDFFF)")

-- Parse four hex digits starting at pos; -1 if any digit is invalid.
readHex4 :: String -> Int -> Int
readHex4 s pos = combineHex (hexVal (strByte s pos)) (hexVal (strByte s (pos + 1))) (hexVal (strByte s (pos + 2))) (hexVal (strByte s (pos + 3)))

combineHex :: Int -> Int -> Int -> Int -> Int
combineHex a b c d = if a < 0 || b < 0 || c < 0 || d < 0 then -1 else ((a * 16 + b) * 16 + c) * 16 + d

hexVal :: Int -> Int
hexVal c = if c >= 48 && c <= 57 then c - 48 else if c >= 97 && c <= 102 then c - 87 else if c >= 65 && c <= 70 then c - 55 else -1

-- Encode a Unicode code point as UTF-8 bytes.
utf8Encode :: Int -> String
utf8Encode cp = if cp < 128
    then strChar cp
    else if cp < 2048
        then strChar (192 + cp `div` 64) <> strChar (128 + cp `mod` 64)
        else if cp < 65536
            then strChar (224 + cp `div` 4096) <> strChar (128 + (cp `div` 64) `mod` 64) <> strChar (128 + cp `mod` 64)
            else strChar (240 + cp `div` 262144) <> strChar (128 + (cp `div` 4096) `mod` 64) <> strChar (128 + (cp `div` 64) `mod` 64) <> strChar (128 + cp `mod` 64)

-- ================================================================
-- Arrays
-- ================================================================

parseArray :: String -> Int -> Int -> JResult
parseArray s pos len = parseArrayStart s (skipWS s (pos + 1) len) len

parseArrayStart :: String -> Int -> Int -> JResult
parseArrayStart s pos len = if pos > len then JErr "Unterminated array" else if strByte s pos == 93 then JOk (JArr []) (skipWS s (pos + 1) len) else parseArrayElems s pos len []

parseArrayElems :: String -> Int -> Int -> [Json] -> JResult
parseArrayElems s pos len acc = case parseValue s pos len of
    JErr e -> JErr e
    JOk val pos2 -> parseArrayNext s pos2 len (val : acc)

parseArrayNext :: String -> Int -> Int -> [Json] -> JResult
parseArrayNext s pos len acc = if pos > len then JErr "Unterminated array" else if strByte s pos == 93 then JOk (JArr (reverse acc)) (skipWS s (pos + 1) len) else if strByte s pos == 44 then parseArrayElems s (skipWS s (pos + 1) len) len acc else JErr ("Expected ',' or ']' at position " <> show pos)

-- ================================================================
-- Objects
-- ================================================================

parseObject :: String -> Int -> Int -> JResult
parseObject s pos len = parseObjStart s (skipWS s (pos + 1) len) len

parseObjStart :: String -> Int -> Int -> JResult
parseObjStart s pos len = if pos > len then JErr "Unterminated object" else if strByte s pos == 125 then JOk (JObj []) (skipWS s (pos + 1) len) else parseObjPairs s pos len []

parseObjPairs :: String -> Int -> Int -> [(String, Json)] -> JResult
parseObjPairs s pos len acc = if pos > len || strByte s pos /= 34 then JErr ("Expected string key at position " <> show pos) else case parseStr s (pos + 1) len of
    SErr e -> JErr e
    SOk key pos2 -> parseObjColon s key pos2 len acc

parseObjColon :: String -> String -> Int -> Int -> [(String, Json)] -> JResult
parseObjColon s key pos len acc = if pos > len || strByte s pos /= 58 then JErr ("Expected ':' at position " <> show pos) else case parseValue s (skipWS s (pos + 1) len) len of
    JErr e -> JErr e
    JOk val pos2 -> parseObjNext s pos2 len ((key, val) : acc)

parseObjNext :: String -> Int -> Int -> [(String, Json)] -> JResult
parseObjNext s pos len acc = if pos > len then JErr "Unterminated object" else if strByte s pos == 125 then JOk (JObj (reverse acc)) (skipWS s (pos + 1) len) else if strByte s pos == 44 then parseObjPairs s (skipWS s (pos + 1) len) len acc else JErr ("Expected ',' or '}' at position " <> show pos)

-- ================================================================
-- Whitespace
-- ================================================================

skipWS :: String -> Int -> Int -> Int
skipWS s pos len = if pos > len then pos else skipWSByte s pos len (strByte s pos)

skipWSByte :: String -> Int -> Int -> Int -> Int
skipWSByte s pos len 32 = skipWS s (pos + 1) len
skipWSByte s pos len 9 = skipWS s (pos + 1) len
skipWSByte s pos len 10 = skipWS s (pos + 1) len
skipWSByte s pos len 13 = skipWS s (pos + 1) len
skipWSByte s pos len _ = pos

-- ================================================================
-- Serializer internals
-- ================================================================

encodeElems :: [Json] -> String
encodeElems [] = ""
encodeElems [x] = encodeJSON x
encodeElems (x:xs) = encodeJSON x <> "," <> encodeElems xs

encodePairs :: [(String, Json)] -> String
encodePairs [] = ""
encodePairs [(k, v)] = encodeStr k <> ":" <> encodeJSON v
encodePairs ((k, v):rest) = encodeStr k <> ":" <> encodeJSON v <> "," <> encodePairs rest

-- Numbers: Lua's integer subtype prints exactly with %d. Floats try %.14g
-- first (shortest common form) and fall back to more digits until the text
-- parses back to the identical double, so encoding never loses precision.
-- NaN and the infinities are not representable in JSON and become null.
encodeNum :: Number -> String
encodeNum n = if n /= n || n == numHuge || n == 0.0 - numHuge
    then "null"
    else if numMathType n == "integer" then numFormat "%d" n else encodeFloat n

encodeFloat :: Number -> String
encodeFloat n = encodeFloat16 n (numFormat "%.14g" n)

encodeFloat16 :: Number -> String -> String
encodeFloat16 n s = if toNumber s == n then s else encodeFloat17 n (numFormat "%.16g" n)

encodeFloat17 :: Number -> String -> String
encodeFloat17 n s = if toNumber s == n then s else numFormat "%.17g" n

-- Strings: " and \ and the control characters are escaped (the short forms
-- \b \f \n \r \t where they exist, \u00XX otherwise); everything else is
-- copied through byte-for-byte, so UTF-8 text stays UTF-8.
encodeStr :: String -> String
encodeStr s = strChar 34 <> escStr s 1 1 (strLen s) "" <> strChar 34

escStr :: String -> Int -> Int -> Int -> String -> String
escStr s chunkStart pos len acc = if pos > len then acc <> strSub s chunkStart (pos - 1) else escStrByte s chunkStart pos len acc (strByte s pos)

escStrByte :: String -> Int -> Int -> Int -> String -> Int -> String
escStrByte s chunkStart pos len acc c = if c == 34 || c == 92 || c < 32
    then escStr s (pos + 1) (pos + 1) len (acc <> strSub s chunkStart (pos - 1) <> escChar c)
    else escStr s chunkStart (pos + 1) len acc

escChar :: Int -> String
escChar 34 = strChar 92 <> strChar 34
escChar 92 = strChar 92 <> strChar 92
escChar 8 = "\\b"
escChar 12 = "\\f"
escChar 10 = "\\n"
escChar 13 = "\\r"
escChar 9 = "\\t"
escChar c = "\\u00" <> hexDigitChar (c `div` 16) <> hexDigitChar (c `mod` 16)

hexDigitChar :: Int -> String
hexDigitChar d = if d < 10 then strChar (48 + d) else strChar (87 + d)

-- Local FFI bindings for the codecs (kept local to avoid polluting the
-- merged namespace with an LMath import).
intToNum :: Int -> LuaPure "tonumber" Number
numModf :: Number -> LuaPure "math.modf" (Number, Number)
numFloor :: Number -> LuaPure "math.floor" Int
numFormat :: String -> Number -> LuaPure "string.format" String
-- __mll_math_type is the runtime's portability shim around math.type: native
-- on Lua 5.3+, and on interpreters without an integer subtype (LuaJIT,
-- Lua 5.1/5.2) it answers "float" for every number — which is the truth
-- there, since all numbers are IEEE-754 doubles.
numMathType :: Number -> LuaPure "__mll_math_type" String
numHuge :: LuaPure "math.huge" Number

-- True when this interpreter distinguishes integer numbers from floats
-- (Lua 5.3+). Where it is False (LuaJIT), numbers are doubles only, so
-- integers beyond 2^53 are not exactly representable and the strict 64-bit
-- range checks in numToIntegerFloat correctly reject the int64 boundaries.
-- note: GHC's Int is a true 64-bit integer on every platform, so this Lua
-- interpreter distinction has no GHC counterpart; use it to gate exactness
-- expectations beyond 2^53 (the LuaJIT double-only host limitation).
hasIntegerSubtype :: Bool
hasIntegerSubtype = numMathType (intToNum 1) == "integer"

-- ================================================================
-- Accessors
-- ================================================================

jLookup :: String -> Json -> Maybe Json
jLookup _ (JObj []) = Nothing
jLookup k (JObj ((fk, fv) : rest)) = if k == fk then Just fv else jLookup k (JObj rest)
jLookup _ _ = Nothing

jIndex :: Int -> Json -> Maybe Json
jIndex _ (JArr []) = Nothing
jIndex 0 (JArr (x:_)) = Just x
jIndex n (JArr (_:xs)) = jIndex (n - 1) (JArr xs)
jIndex _ _ = Nothing

jString :: Json -> Maybe String
jString (JStr s) = Just s
jString _ = Nothing

jNumber :: Json -> Maybe Number
jNumber (JNum n) = Just n
jNumber _ = Nothing

jBool :: Json -> Maybe Bool
jBool (JBool b) = Just b
jBool _ = Nothing

jIsNull :: Json -> Bool
jIsNull JNull = True
jIsNull _ = False

-- ================================================================
-- Primitive ToJSON instances
--
-- With orphan checking relaxed for library modules, the JSON module can
-- carry the ToJSON instances for builtin types that the generic encoder's
-- leaves (`K1 c`) resolve against. They wrap the same combinators the native
-- derive uses, so `toJSON` and the derived encoders agree.
-- ================================================================

instance ToJSON Int where
    toJSON n = toJSONInt n

instance ToJSON Number where
    toJSON n = toJSONNumber n

instance ToJSON String where
    toJSON s = toJSONString s

instance ToJSON Bool where
    toJSON b = toJSONBool b

instance ToJSON Json where
    toJSON j = toJSONValue j

instance ToJSON a => ToJSON [a] where
    toJSON xs = toJSONList toJSON xs

instance ToJSON a => ToJSON (Maybe a) where
    toJSON m = toJSONMaybe toJSON m

-- ================================================================
-- Primitive FromJSON instances
--
-- The decode-side twins: the generic decoder's leaves (`K1 c`) resolve
-- against these. They wrap the same combinators the native derive uses, so
-- `fromJSON` and the derived decoders agree — including error messages.
-- The Maybe instance overrides fromJSONField: a Maybe RECORD FIELD is
-- optional (missing key or null -> Nothing), which is a property of the
-- field lookup, not of the value decoder.
-- ================================================================

instance FromJSON Int where
    fromJSON j = fromJSONInt j

instance FromJSON Number where
    fromJSON j = fromJSONNumber j

instance FromJSON String where
    fromJSON j = fromJSONString j

instance FromJSON Bool where
    fromJSON j = fromJSONBool j

instance FromJSON Json where
    fromJSON j = fromJSONValue j

instance FromJSON a => FromJSON [a] where
    fromJSON j = fromJSONList fromJSON j

instance FromJSON a => FromJSON (Maybe a) where
    fromJSON j = fromJSONMaybe fromJSON j
    fromJSONField k j = jOptFieldWith fromJSON k j

-- ================================================================
-- Generic ToJSON  (import Data.Generics)
--
-- `genericToJSON` encodes any `deriving (Generic)` type by walking its
-- representation, reproducing the wire format of `deriving (ToJSON)`
-- byte-for-byte: a single-constructor record is an object keyed by field
-- name; a single positional constructor is its argument (or an array); a
-- multi-constructor type is tagged (a nullary constructor is the bare name
-- string, a fielded one an object with "tag"); `Maybe` fields encode
-- Nothing as null. The record/sum/arity decisions are read from the derived
-- metadata as values, so one instance per combinator suffices.
-- ================================================================

-- Assemble one constructor's JSON from its name, arity, record-ness, whether
-- the datatype tags its constructors, and the encoded fields (name + value).
encodeCon :: String -> Int -> Bool -> Bool -> [(String, Json)] -> Json
encodeCon nm ar isRec tagged fields =
    if ar == 0
        then JStr nm
        else if isRec
            then (if tagged then JObj (("tag", JStr nm) : fields) else JObj fields)
            else genEncodePositional nm ar tagged (map snd fields)

genEncodePositional :: String -> Int -> Bool -> [Json] -> Json
genEncodePositional nm ar tagged vals =
    let contents = if ar == 1 then genHead vals else JArr vals
    in if tagged
        then JObj (("tag", JStr nm) : ("contents", contents) : [])
        else contents

genHead :: [Json] -> Json
genHead (x : _) = x
genHead [] = JNull

class GEncode f where
    gEncode :: f -> Json

class GSum f where
    gSum :: Bool -> f -> Json

class GFields f where
    gFields :: f -> [(String, Json)]

-- The field leaf: a `K1` holding one value, encoded by its own ToJSON instance.
class GLeaf f where
    gLeaf :: f -> Json

genericToJSON :: (Generic a, GEncode (Rep a)) => a -> Json
genericToJSON x = gEncode (from x)

instance (Datatype d, GSum f) => GEncode (D1 d f) where
    gEncode d1 = case d1 of
        D1 y -> gSum (datatypeConCount d1 > 1) y

instance (GSum a, GSum b) => GSum (a :+: b) where
    gSum t (L1 x) = gSum t x
    gSum t (R1 y) = gSum t y

instance (Constructor c, GFields f) => GSum (C1 c f) where
    gSum tagged c1 = case c1 of
        C1 y -> encodeCon (conName c1) (conArity c1) (conIsRecord c1) tagged (gFields y)

instance GFields U1 where
    gFields _ = []

instance (GFields a, GFields b) => GFields (a :*: b) where
    gFields (Prod a b) = gFields a ++ gFields b

instance (Selector s, GLeaf f) => GFields (S1 s f) where
    gFields s1 = case s1 of
        S1 y -> (selName s1, gLeaf y) : []

instance ToJSON c => GLeaf (K1 c) where
    gLeaf k1 = case k1 of
        K1 v -> toJSON v

-- ================================================================
-- Generic FromJSON  (import Data.Generics)
--
-- `genericFromJSON` decodes any `deriving (Generic)` type by walking its
-- representation TYPE, reproducing the wire format and the error messages
-- of `deriving (FromJSON)` exactly (it calls the same combinators with the
-- same strings) — the exact mirror of genericToJSON, so encode/decode
-- round-trips. Decoding must pick instances before any rep value exists,
-- so it navigates by proxy (`gProxy` and the `p*` re-typers from
-- Data.Generics) — the proxies are never forced; only their types matter.
-- ================================================================

-- Either map, specialized to the decoder's error type.
gdMap :: (a -> b) -> Either String a -> Either String b
gdMap _ (Left e) = Left e
gdMap f (Right x) = Right (f x)

-- "'A', 'B' or 'C'" — the expected-tags list of jBadTag, exactly as the
-- native derive formats it.
gdExpected :: [String] -> String
gdExpected [] = ""
gdExpected (n : []) = "'" <> n <> "'"
gdExpected (n : rest) = gdExpectedGo ("'" <> n <> "'") rest

gdExpectedGo :: String -> [String] -> String
gdExpectedGo acc [] = acc
gdExpectedGo acc (n : []) = acc <> " or '" <> n <> "'"
gdExpectedGo acc (n : rest) = gdExpectedGo (acc <> ", '" <> n <> "'") rest

-- The whole-datatype layer.
class GDecode f where
    gDecode :: f -> Json -> Either String f

-- The constructor-choice layer. gdStr/gdTag return Nothing when the given
-- tag names no constructor of this part of the sum — the D1 layer turns
-- that into jBadTag with the full expected list.
class GDecSum f where
    gdStr :: f -> String -> Maybe (Either String f)
    gdTag :: f -> String -> Json -> Maybe (Either String f)
    gdUntagged :: f -> Json -> Either String f
    gdConNames :: f -> [String]
    gdSoleArity :: f -> Int

-- The fields-of-one-constructor layer.
class GDecProd f where
    gdpNullary :: f -> Either String f
    gdpCount :: f -> Int
    gdpRecord :: f -> Json -> Either String f
    gdpArgs :: f -> String -> Int -> [Json] -> Either String f

-- The field leaf: a `K1` holding one value, decoded by its own FromJSON
-- instance (fromJSONField for record lookup, fromJSON for a bare value).
class GDecLeaf f where
    gdlField :: f -> String -> Json -> Either String f
    gdlValue :: f -> Json -> Either String f

genericFromJSON :: (Generic a, GDecode (Rep a)) => Json -> Either String a
genericFromJSON j = case gDecode gProxy j of
    Left e -> Left e
    Right r -> Right (to r)

instance (Datatype d, GDecSum f) => GDecode (D1 d f) where
    gDecode p j =
        jContext (datatypeName p)
            (gdMap D1 (gdBody (pD1 p) (datatypeConCount p) j))

-- Tagged decoding applies to multi-constructor types, and to a lone
-- nullary constructor (the constructor NAME is the payload) — the same
-- rule the native derive applies.
gdBody :: GDecSum f => f -> Int -> Json -> Either String f
gdBody p n j =
    if n > 1 || gdSoleArity p == 0
        then gdTagged p j
        else gdUntagged p j

gdTagged :: GDecSum f => f -> Json -> Either String f
gdTagged p j = case j of
    JStr s -> case gdStr p s of
        Just r -> r
        Nothing -> jBadTag (gdExpected (gdConNames p)) s
    JObj _ -> case jFieldWith fromJSONString "tag" j of
        Left e -> Left e
        Right tag -> case gdTag p tag j of
            Just r -> r
            Nothing -> jBadTag (gdExpected (gdConNames p)) tag
    _ -> jExpectTagged j

instance (GDecSum a, GDecSum b) => GDecSum (a :+: b) where
    gdStr p s = case gdStr (pSumL p) s of
        Just r -> Just (gdMap L1 r)
        Nothing -> case gdStr (pSumR p) s of
            Just r -> Just (gdMap R1 r)
            Nothing -> Nothing
    gdTag p t j = case gdTag (pSumL p) t j of
        Just r -> Just (gdMap L1 r)
        Nothing -> case gdTag (pSumR p) t j of
            Just r -> Just (gdMap R1 r)
            Nothing -> Nothing
    gdUntagged _ _ = Left "unreachable: untagged decode of a multi-constructor type"
    gdConNames p = gdConNames (pSumL p) ++ gdConNames (pSumR p)
    gdSoleArity _ = 1

-- Decode one constructor's payload from the tagged object `j`: record
-- fields inline in the object, positional arguments under "contents" (the
-- value itself for one argument, an array for several), nothing needed for
-- a nullary constructor.
gdConBody :: (Constructor c, GDecProd f) => C1 c f -> Json -> Either String (C1 c f)
gdConBody p j =
    if conArity p == 0
        then gdMap C1 (gdpNullary (pC1 p))
        else if conIsRecord p
            then gdMap C1 (gdpRecord (pC1 p) j)
            else case jField "contents" j of
                Left e -> Left e
                Right c -> if conArity p == 1
                    then gdMap C1 (gdpArgs (pC1 p) (conName p) 0 (c : []))
                    else case jExpectArrN (conName p) (conArity p) c of
                        Left e -> Left e
                        Right xs -> gdMap C1 (gdpArgs (pC1 p) (conName p) 0 xs)

instance (Constructor c, GDecProd f) => GDecSum (C1 c f) where
    gdStr p s =
        if s == conName p
            then Just (if conArity p == 0
                then gdMap C1 (gdpNullary (pC1 p))
                else jTagNeedsObject (conName p))
            else Nothing
    gdTag p t j = if t == conName p then Just (gdConBody p j) else Nothing
    gdUntagged p j =
        if conIsRecord p
            then gdMap C1 (gdpRecord (pC1 p) j)
            else if conArity p == 1
                then gdMap C1 (gdpArgs (pC1 p) (conName p) 0 (j : []))
                else case jExpectArrN (conName p) (conArity p) j of
                    Left e -> Left e
                    Right xs -> gdMap C1 (gdpArgs (pC1 p) (conName p) 0 xs)
    gdConNames p = conName p : []
    gdSoleArity p = conArity p

instance GDecProd U1 where
    gdpNullary _ = Right U1
    gdpCount _ = 0
    gdpRecord _ _ = Right U1
    gdpArgs _ _ _ _ = Right U1

instance (Selector s, GDecLeaf f) => GDecProd (S1 s f) where
    gdpNullary _ = Left "unreachable: a fielded constructor decoded as nullary"
    gdpCount _ = 1
    gdpRecord p j = gdMap S1 (gdlField (pS1 p) (selName p) j)
    gdpArgs p con i elems = gdMap S1 (jArgWith (gdlValue (pS1 p)) con (i + 1) (jNth 0 elems))

instance (GDecProd a, GDecProd b) => GDecProd (a :*: b) where
    gdpNullary _ = Left "unreachable: a fielded constructor decoded as nullary"
    gdpCount p = gdpCount (pProdL p) + gdpCount (pProdR p)
    gdpRecord p j = case gdpRecord (pProdL p) j of
        Left e -> Left e
        Right x -> gdMap (\y -> Prod x y) (gdpRecord (pProdR p) j)
    gdpArgs p con i elems =
        let nl = gdpCount (pProdL p)
        in case gdpArgs (pProdL p) con i (take nl elems) of
            Left e -> Left e
            Right x -> gdMap (\y -> Prod x y) (gdpArgs (pProdR p) con (i + nl) (drop nl elems))

instance FromJSON c => GDecLeaf (K1 c) where
    gdlField _ k j = gdMap K1 (fromJSONField k j)
    gdlValue _ j = gdMap K1 (fromJSON j)
