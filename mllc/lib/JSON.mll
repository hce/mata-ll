import LString (strByte, strLen, strSub, strChar)

-- JSON value type
data Json = JNull | JBool Bool | JNum Number | JStr String | JArr [Json] | JObj [(String, Json)]
    deriving (Eq)

-- Parse result
data JResult = JOk Json Integer | JErr String

-- Internal result for strings (avoids wrapping in Json)
data SResult = SOk String Integer | SErr String

-- Internal result for arrays
data AResult = AOk [Json] Integer | AErr String

-- Internal result for object pairs
data OResult = OOk [(String, Json)] Integer | OErr String

-- ================================================================
-- Public API
-- ================================================================

parseJSON :: String -> Either String Json
parseJSON s = parseTop s (strLen s)

parseTop :: String -> Integer -> Either String Json
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
-- The primitive codecs live in the toJSON*/fromJSON* combinators below
-- rather than in `instance ToJSON Integer`-style declarations: mata-ll
-- rejects an instance declared in an imported module as an orphan (only the
-- main module's own classes/types count as local), so a stdlib module cannot
-- carry instances for builtin types. Write instances for your own data types
-- in terms of these combinators; a derived FromJSON/ToJSON will call the
-- same combinators.
-- ================================================================

class ToJSON a where
    toJSON :: a -> Json

class FromJSON a where
    fromJSON :: Json -> Either String a

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

toJSONInteger :: Integer -> Json
toJSONInteger n = JNum (intToNum n)

-- Decode a number that must be integral. A non-integral number (3.5) and a
-- number outside the 64-bit Integer range are both rejected with a clear error.
fromJSONInteger :: Json -> Either String Integer
fromJSONInteger (JNum n) = numToInteger n
fromJSONInteger j = Left ("expected an integer, but found " <> jTypeName j)

-- A number with Lua's integer subtype is exact and in range by construction
-- (the parser only produces one for in-range integer syntax). Floats go
-- through the integrality and range checks.
numToInteger :: Number -> Either String Integer
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
numToIntegerFloat :: Number -> Either String Integer
numToIntegerFloat n = case numModf n of
    (_, frac) -> if frac /= 0.0
        then Left ("expected an integer, but found the non-integral number " <> encodeNum n)
        else if n > -9223372036854775808.0 && n < 9223372036854775808.0
            then Right (numFloor n)
            else Left ("the number " <> encodeNum n <> " is outside the 64-bit Integer range")

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

decodeElems :: (Json -> Either String a) -> [Json] -> Integer -> Either String [a]
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
-- Combinators for derived FromJSON decoders
--
-- `deriving (FromJSON)` generates a decoder over these. The convention
-- (mirroring aeson's defaultOptions where mata-ll can):
--   * a single-constructor record decodes from an object keyed by the
--     field names:            data P = P { x :: Integer }   ⇐  {"x":1}
--   * a single positional constructor decodes from its argument itself
--     (one field) or an array of its arguments (several fields):
--                             data W = W Integer            ⇐  7
--                             data V = V Integer String     ⇐  [7,"a"]
--   * a multi-constructor type is tagged: either the bare constructor
--     name as a string (nullary constructors only), or an object with a
--     "tag" field — record fields inline in the same object, positional
--     arguments under "contents":
--                             data S = A | B Integer | C { n :: Integer }
--                               ⇐  "A"  or  {"tag":"A"}
--                               ⇐  {"tag":"B","contents":7}
--                               ⇐  {"tag":"C","n":3}
--   * a Maybe field decodes from a missing key, null, or the value itself.
--   * unknown object keys are ignored, as aeson does.
--
-- note: aeson encodes only ALL-nullary sum types as bare strings; the
-- derived decoder accepts both the bare string and the tagged-object form
-- for any nullary constructor, since a reasonable encoder may emit either.
-- note: Maybe (Maybe a) cannot round-trip under the null-is-Nothing
-- convention — Just Nothing has no JSON form distinct from Nothing.
-- ================================================================

-- Identity decoder: keep the raw Json value (for fields of type Json).
fromJSONValue :: Json -> Either String Json
fromJSONValue j = Right j

-- The n-th element (0-based) of a list of Json values. Total: out of range
-- yields JNull, but derived decoders only index after jExpectArrN has
-- checked the arity.
jNth :: Integer -> [Json] -> Json
jNth _ [] = JNull
jNth 0 (x:_) = x
jNth n (_:xs) = jNth (n - 1) xs

-- Expect the positional arguments of constructor `con` as an array of
-- exactly n elements.
jExpectArrN :: String -> Integer -> Json -> Either String [Json]
jExpectArrN con n j = case jExpectArr j of
    Left e -> Left ("in the arguments of constructor '" <> con <> "': " <> e)
    Right xs -> if length xs == n
        then Right xs
        else Left ("constructor '" <> con <> "' takes " <> show n <> " arguments, but the array has " <> show (length xs))

-- Decode argument #i (1-based, for messages) of constructor `con`.
jArgWith :: (Json -> Either String a) -> String -> Integer -> Json -> Either String a
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

parseValue :: String -> Integer -> Integer -> JResult
parseValue s pos len = if pos > len then JErr "Unexpected end of input" else dispatchValue s pos len (strByte s pos)

dispatchValue :: String -> Integer -> Integer -> Integer -> JResult
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

parseNull :: String -> Integer -> Integer -> JResult
parseNull s pos len = if pos + 3 <= len && strSub s pos (pos + 3) == "null" then JOk JNull (skipWS s (pos + 4) len) else JErr "Expected 'null'"

parseTrue :: String -> Integer -> Integer -> JResult
parseTrue s pos len = if pos + 3 <= len && strSub s pos (pos + 3) == "true" then JOk (JBool True) (skipWS s (pos + 4) len) else JErr "Expected 'true'"

parseFalse :: String -> Integer -> Integer -> JResult
parseFalse s pos len = if pos + 4 <= len && strSub s pos (pos + 4) == "false" then JOk (JBool False) (skipWS s (pos + 5) len) else JErr "Expected 'false'"

-- ================================================================
-- Numbers
--
-- Follows the JSON grammar exactly: -?int frac? exp?, where int has no
-- leading zeros, and frac/exp require at least one digit. Enforcing the
-- grammar here guarantees the matched text is always a valid Lua numeral,
-- so toNumber can never return garbage. Integer-syntax numbers parse to
-- Lua's 64-bit integer subtype (exact for the full Integer range) and
-- float syntax parses to a double, so no information is lost that a later
-- integrality check (fromJSONInteger) would need.
-- ================================================================

parseNumber :: String -> Integer -> Integer -> JResult
parseNumber s pos len = if strByte s pos == 45 then numIntStart s (pos + 1) len pos else numIntStart s pos len pos

numIntStart :: String -> Integer -> Integer -> Integer -> JResult
numIntStart s pos len start = if pos > len then JErr ("Invalid number at position " <> show start <> ": expected a digit") else numIntByte s pos len start (strByte s pos)

numIntByte :: String -> Integer -> Integer -> Integer -> Integer -> JResult
numIntByte s pos len start 48 = numAfterInt s (pos + 1) len start
numIntByte s pos len start c = if c >= 49 && c <= 57 then numDigits s (pos + 1) len start else JErr ("Invalid number at position " <> show start <> ": expected a digit")

numDigits :: String -> Integer -> Integer -> Integer -> JResult
numDigits s pos len start = if pos <= len && isDigitByte (strByte s pos) then numDigits s (pos + 1) len start else numAfterInt s pos len start

numAfterInt :: String -> Integer -> Integer -> Integer -> JResult
numAfterInt s pos len start = if pos > len then numFinish s start pos else numAfterIntByte s pos len start (strByte s pos)

numAfterIntByte :: String -> Integer -> Integer -> Integer -> Integer -> JResult
numAfterIntByte s pos len start 46 = numFracStart s (pos + 1) len start
numAfterIntByte s pos len start 101 = numExpSign s (pos + 1) len start
numAfterIntByte s pos len start 69 = numExpSign s (pos + 1) len start
numAfterIntByte s pos len start c = if isDigitByte c then JErr ("Invalid number at position " <> show start <> ": JSON does not allow leading zeros") else numFinish s start pos

numFracStart :: String -> Integer -> Integer -> Integer -> JResult
numFracStart s pos len start = if pos <= len && isDigitByte (strByte s pos) then numFracDigits s (pos + 1) len start else JErr ("Invalid number at position " <> show start <> ": at least one digit is required after the decimal point")

numFracDigits :: String -> Integer -> Integer -> Integer -> JResult
numFracDigits s pos len start = if pos <= len && isDigitByte (strByte s pos) then numFracDigits s (pos + 1) len start else numAfterFrac s pos len start

numAfterFrac :: String -> Integer -> Integer -> Integer -> JResult
numAfterFrac s pos len start = if pos <= len && (strByte s pos == 101 || strByte s pos == 69) then numExpSign s (pos + 1) len start else numFinish s start pos

numExpSign :: String -> Integer -> Integer -> Integer -> JResult
numExpSign s pos len start = if pos <= len && (strByte s pos == 43 || strByte s pos == 45) then numExpStart s (pos + 1) len start else numExpStart s pos len start

numExpStart :: String -> Integer -> Integer -> Integer -> JResult
numExpStart s pos len start = if pos <= len && isDigitByte (strByte s pos) then numExpDigits s (pos + 1) len start else JErr ("Invalid number at position " <> show start <> ": at least one digit is required in the exponent")

numExpDigits :: String -> Integer -> Integer -> Integer -> JResult
numExpDigits s pos len start = if pos <= len && isDigitByte (strByte s pos) then numExpDigits s (pos + 1) len start else numFinish s start pos

numFinish :: String -> Integer -> Integer -> JResult
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

isDigitByte :: Integer -> Bool
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

parseStringVal :: String -> Integer -> Integer -> JResult
parseStringVal s pos len = case parseStr s (pos + 1) len of
    SErr e -> JErr e
    SOk str pos2 -> JOk (JStr str) pos2

parseStr :: String -> Integer -> Integer -> SResult
parseStr s pos len = scanStr s len pos pos ""

scanStr :: String -> Integer -> Integer -> Integer -> String -> SResult
scanStr s len chunkStart pos acc = if pos > len then SErr "Unterminated string" else scanStrByte s len chunkStart pos acc (strByte s pos)

scanStrByte :: String -> Integer -> Integer -> Integer -> String -> Integer -> SResult
scanStrByte s len chunkStart pos acc 34 = SOk (acc <> strSub s chunkStart (pos - 1)) (skipWS s (pos + 1) len)
scanStrByte s len chunkStart pos acc 92 = scanEsc s len (pos + 1) (acc <> strSub s chunkStart (pos - 1))
scanStrByte s len chunkStart pos acc c = if c < 32 then SErr ("Control character (byte " <> show c <> ") at position " <> show pos <> ": control characters must be escaped inside a JSON string") else scanStr s len chunkStart (pos + 1) acc

-- pos is the character after the backslash.
scanEsc :: String -> Integer -> Integer -> String -> SResult
scanEsc s len pos acc = if pos > len then SErr "Unterminated escape sequence" else scanEscByte s len pos acc (strByte s pos)

scanEscByte :: String -> Integer -> Integer -> String -> Integer -> SResult
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
scanU :: String -> Integer -> Integer -> String -> SResult
scanU s len pos acc = if pos + 4 > len then SErr ("Truncated \\u escape at position " <> show pos <> ": four hex digits are required") else scanUCode s len pos acc (readHex4 s (pos + 1))

scanUCode :: String -> Integer -> Integer -> String -> Integer -> SResult
scanUCode s len pos acc cp = if cp < 0
    then SErr ("Invalid \\u escape at position " <> show pos <> ": four hex digits are required")
    else if cp >= 55296 && cp <= 56319
        then scanUPair s len pos acc cp
        else if cp >= 56320 && cp <= 57343
            then SErr ("Lone low surrogate \\u escape at position " <> show pos <> ": a low surrogate (\\uDC00-\\uDFFF) is only valid immediately after a high surrogate")
            else scanStr s len (pos + 5) (pos + 5) (acc <> utf8Encode cp)

-- A high surrogate must be followed by \uXXXX with a low surrogate; together
-- they encode one code point above U+FFFF. The low escape spans pos+5..pos+10.
scanUPair :: String -> Integer -> Integer -> String -> Integer -> SResult
scanUPair s len pos acc hi = if pos + 10 > len || strByte s (pos + 5) /= 92 || strByte s (pos + 6) /= 117
    then loneHighErr pos
    else scanUPairLow s len pos acc hi (readHex4 s (pos + 7))

scanUPairLow :: String -> Integer -> Integer -> String -> Integer -> Integer -> SResult
scanUPairLow s len pos acc hi lo = if lo >= 56320 && lo <= 57343
    then scanStr s len (pos + 11) (pos + 11) (acc <> utf8Encode (65536 + (hi - 55296) * 1024 + (lo - 56320)))
    else loneHighErr pos

loneHighErr :: Integer -> SResult
loneHighErr pos = SErr ("Unpaired high surrogate \\u escape at position " <> show pos <> ": JSON encodes characters above U+FFFF as a surrogate pair, so a high surrogate (\\uD800-\\uDBFF) must be followed immediately by a low surrogate (\\uDC00-\\uDFFF)")

-- Parse four hex digits starting at pos; -1 if any digit is invalid.
readHex4 :: String -> Integer -> Integer
readHex4 s pos = combineHex (hexVal (strByte s pos)) (hexVal (strByte s (pos + 1))) (hexVal (strByte s (pos + 2))) (hexVal (strByte s (pos + 3)))

combineHex :: Integer -> Integer -> Integer -> Integer -> Integer
combineHex a b c d = if a < 0 || b < 0 || c < 0 || d < 0 then -1 else ((a * 16 + b) * 16 + c) * 16 + d

hexVal :: Integer -> Integer
hexVal c = if c >= 48 && c <= 57 then c - 48 else if c >= 97 && c <= 102 then c - 87 else if c >= 65 && c <= 70 then c - 55 else -1

-- Encode a Unicode code point as UTF-8 bytes.
utf8Encode :: Integer -> String
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

parseArray :: String -> Integer -> Integer -> JResult
parseArray s pos len = parseArrayStart s (skipWS s (pos + 1) len) len

parseArrayStart :: String -> Integer -> Integer -> JResult
parseArrayStart s pos len = if pos > len then JErr "Unterminated array" else if strByte s pos == 93 then JOk (JArr []) (skipWS s (pos + 1) len) else parseArrayElems s pos len []

parseArrayElems :: String -> Integer -> Integer -> [Json] -> JResult
parseArrayElems s pos len acc = case parseValue s pos len of
    JErr e -> JErr e
    JOk val pos2 -> parseArrayNext s pos2 len (val : acc)

parseArrayNext :: String -> Integer -> Integer -> [Json] -> JResult
parseArrayNext s pos len acc = if pos > len then JErr "Unterminated array" else if strByte s pos == 93 then JOk (JArr (reverse acc)) (skipWS s (pos + 1) len) else if strByte s pos == 44 then parseArrayElems s (skipWS s (pos + 1) len) len acc else JErr ("Expected ',' or ']' at position " <> show pos)

-- ================================================================
-- Objects
-- ================================================================

parseObject :: String -> Integer -> Integer -> JResult
parseObject s pos len = parseObjStart s (skipWS s (pos + 1) len) len

parseObjStart :: String -> Integer -> Integer -> JResult
parseObjStart s pos len = if pos > len then JErr "Unterminated object" else if strByte s pos == 125 then JOk (JObj []) (skipWS s (pos + 1) len) else parseObjPairs s pos len []

parseObjPairs :: String -> Integer -> Integer -> [(String, Json)] -> JResult
parseObjPairs s pos len acc = if pos > len || strByte s pos /= 34 then JErr ("Expected string key at position " <> show pos) else case parseStr s (pos + 1) len of
    SErr e -> JErr e
    SOk key pos2 -> parseObjColon s key pos2 len acc

parseObjColon :: String -> String -> Integer -> Integer -> [(String, Json)] -> JResult
parseObjColon s key pos len acc = if pos > len || strByte s pos /= 58 then JErr ("Expected ':' at position " <> show pos) else case parseValue s (skipWS s (pos + 1) len) len of
    JErr e -> JErr e
    JOk val pos2 -> parseObjNext s pos2 len ((key, val) : acc)

parseObjNext :: String -> Integer -> Integer -> [(String, Json)] -> JResult
parseObjNext s pos len acc = if pos > len then JErr "Unterminated object" else if strByte s pos == 125 then JOk (JObj (reverse acc)) (skipWS s (pos + 1) len) else if strByte s pos == 44 then parseObjPairs s (skipWS s (pos + 1) len) len acc else JErr ("Expected ',' or '}' at position " <> show pos)

-- ================================================================
-- Whitespace
-- ================================================================

skipWS :: String -> Integer -> Integer -> Integer
skipWS s pos len = if pos > len then pos else skipWSByte s pos len (strByte s pos)

skipWSByte :: String -> Integer -> Integer -> Integer -> Integer
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

escStr :: String -> Integer -> Integer -> Integer -> String -> String
escStr s chunkStart pos len acc = if pos > len then acc <> strSub s chunkStart (pos - 1) else escStrByte s chunkStart pos len acc (strByte s pos)

escStrByte :: String -> Integer -> Integer -> Integer -> String -> Integer -> String
escStrByte s chunkStart pos len acc c = if c == 34 || c == 92 || c < 32
    then escStr s (pos + 1) (pos + 1) len (acc <> strSub s chunkStart (pos - 1) <> escChar c)
    else escStr s chunkStart (pos + 1) len acc

escChar :: Integer -> String
escChar 34 = strChar 92 <> strChar 34
escChar 92 = strChar 92 <> strChar 92
escChar 8 = "\\b"
escChar 12 = "\\f"
escChar 10 = "\\n"
escChar 13 = "\\r"
escChar 9 = "\\t"
escChar c = "\\u00" <> hexDigitChar (c `div` 16) <> hexDigitChar (c `mod` 16)

hexDigitChar :: Integer -> String
hexDigitChar d = if d < 10 then strChar (48 + d) else strChar (87 + d)

-- Local FFI bindings for the codecs (kept local to avoid polluting the
-- merged namespace with an LMath import).
intToNum :: Integer -> LuaPure "tonumber" Number
numModf :: Number -> LuaPure "math.modf" (Number, Number)
numFloor :: Number -> LuaPure "math.floor" Integer
numFormat :: String -> Number -> LuaPure "string.format" String
numMathType :: Number -> LuaPure "math.type" String
numHuge :: LuaPure "math.huge" Number

-- ================================================================
-- Accessors
-- ================================================================

jLookup :: String -> Json -> Maybe Json
jLookup _ (JObj []) = Nothing
jLookup k (JObj ((fk, fv) : rest)) = if k == fk then Just fv else jLookup k (JObj rest)
jLookup _ _ = Nothing

jIndex :: Integer -> Json -> Maybe Json
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
