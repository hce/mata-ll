-- JSON: a recursive-descent parser and a serializer over character codes.
-- Leans on: LString character access, mutually recursive parsers over
-- Either with early exit, a recursive ADT with derived Show (record-free,
-- Number and escaped String fields), Number arithmetic from digit codes,
-- string building with mconcat/<>, Maybe lookups.
import LString
import Data.Maybe (fromMaybe)

data JValue
    = JNull
    | JBool Bool
    | JNumber Number
    | JString String
    | JArray [JValue]
    | JObject [(String, JValue)]
    deriving (Show, Eq)

type Result a = Either String (a, [Int])

-- Character codes (there are no character literals).
cSpace :: Int
cSpace = 32
cQuote :: Int
cQuote = 34
cComma :: Int
cComma = 44
cMinus :: Int
cMinus = 45
cDot :: Int
cDot = 46
cZero :: Int
cZero = 48
cColon :: Int
cColon = 58
cLBracket :: Int
cLBracket = 91
cBackslash :: Int
cBackslash = 92
cRBracket :: Int
cRBracket = 93
cLBrace :: Int
cLBrace = 123
cRBrace :: Int
cRBrace = 125

isWs :: Int -> Bool
isWs c = c == cSpace || c == 9 || c == 10 || c == 13

isDigit :: Int -> Bool
isDigit c = c >= cZero && c <= cZero + 9

skipWs :: [Int] -> [Int]
skipWs (c : cs) | isWs c = skipWs cs
skipWs cs = cs

fromCodes :: [Int] -> String
fromCodes cs = mconcat (map strChar cs)

-- Match a literal word and yield the given value.
literal :: String -> JValue -> [Int] -> Result JValue
literal word v input =
    let codes = strToInts word
        n = length codes
    in if take n input == codes
        then Right (v, drop n input)
        else Left ("expected " <> word)

parseValue :: [Int] -> Result JValue
parseValue input = case skipWs input of
    [] -> Left "unexpected end of input"
    (c : cs)
        | c == 110 -> literal "null" JNull (c : cs)
        | c == 116 -> literal "true" (JBool True) (c : cs)
        | c == 102 -> literal "false" (JBool False) (c : cs)
        | c == cQuote -> case parseString cs of
            Left e -> Left e
            Right (s, rest) -> Right (JString s, rest)
        | c == cLBracket -> parseArray (skipWs cs)
        | c == cLBrace -> parseObject (skipWs cs)
        | c == cMinus || isDigit c -> parseNumber (c : cs)
        | otherwise -> Left ("unexpected character code " <> show c)

-- After the opening quote: collect codes up to the closing quote,
-- decoding the escapes \" \\ \/ \n \t \r.
parseString :: [Int] -> Result String
parseString = go []
  where
    go acc [] = Left "unterminated string"
    go acc (c : cs)
        | c == cQuote = Right (fromCodes (reverse acc), cs)
        | c == cBackslash = case cs of
            [] -> Left "dangling escape"
            (e : rest)
                | e == 110 -> go (10 : acc) rest
                | e == 116 -> go (9 : acc) rest
                | e == 114 -> go (13 : acc) rest
                | e == cQuote || e == cBackslash || e == 47 -> go (e : acc) rest
                | otherwise -> Left ("bad escape code " <> show e)
        | otherwise = go (c : acc) cs

-- Digits are folded into an Integer mantissa; the value is assembled at
-- Number so 1.5e2 and 150 print the same way.
parseNumber :: [Int] -> Result JValue
parseNumber input =
    let (neg, afterSign) = case input of
            (c : cs) | c == cMinus -> (True, cs)
            cs -> (False, cs)
        (intDigits, afterInt) = span isDigit afterSign
        (fracDigits, afterFrac) = case afterInt of
            (c : cs) | c == cDot -> span isDigit cs
            cs -> ([], cs)
        (expo, afterExp) = case afterFrac of
            (c : cs) | c == 101 || c == 69 -> parseExponent cs
            cs -> (0, cs)
    in if null intDigits
        then Left "bad number"
        else
            let mantissa = digitsToInteger (intDigits ++ fracDigits)
                scale = expo - length fracDigits
                magnitude = applyScale (fromInteger mantissa) scale
                value = if neg then negate magnitude else magnitude
            in Right (JNumber value, afterExp)

parseExponent :: [Int] -> (Int, [Int])
parseExponent cs =
    let (sign, body) = case cs of
            (c : rest) | c == cMinus -> (-1, rest)
            (c : rest) | c == 43 -> (1, rest)
            rest -> (1, rest)
        (ds, after) = span isDigit body
    in (sign * fromInteger (digitsToInteger ds), after)

digitsToInteger :: [Int] -> Integer
digitsToInteger = foldl (\acc d -> acc * 10 + toInteger (d - cZero)) 0

applyScale :: Number -> Int -> Number
applyScale x k
    | k > 0 = applyScale (x * 10) (k - 1)
    | k < 0 = applyScale (x / 10) (k + 1)
    | otherwise = x

parseArray :: [Int] -> Result JValue
parseArray (c : cs) | c == cRBracket = Right (JArray [], cs)
parseArray input = go [] input
  where
    go acc cs = case parseValue cs of
        Left e -> Left e
        Right (v, rest) -> case skipWs rest of
            (d : ds)
                | d == cComma -> go (v : acc) ds
                | d == cRBracket -> Right (JArray (reverse (v : acc)), ds)
            _ -> Left "expected , or ] in array"

parseObject :: [Int] -> Result JValue
parseObject (c : cs) | c == cRBrace = Right (JObject [], cs)
parseObject input = go [] input
  where
    go acc cs = case skipWs cs of
        (q : qs) | q == cQuote -> case parseString qs of
            Left e -> Left e
            Right (key, afterKey) -> case skipWs afterKey of
                (d : ds) | d == cColon -> case parseValue ds of
                    Left e -> Left e
                    Right (v, rest) -> case skipWs rest of
                        (e : es)
                            | e == cComma -> go ((key, v) : acc) es
                            | e == cRBrace -> Right (JObject (reverse ((key, v) : acc)), es)
                        _ -> Left "expected , or } in object"
                _ -> Left "expected : after key"
        _ -> Left "expected string key"

parseJson :: String -> Either String JValue
parseJson text = case parseValue (strToInts text) of
    Left e -> Left e
    Right (v, rest) -> case skipWs rest of
        [] -> Right v
        _ -> Left "trailing input"

-- Serializer: compact, with the same escapes the parser accepts.
render :: JValue -> String
render JNull = "null"
render (JBool True) = "true"
render (JBool False) = "false"
render (JNumber n) = show n
render (JString s) = renderString s
render (JArray vs) = "[" <> joinWith "," (map render vs) <> "]"
render (JObject kvs) = "{" <> joinWith "," (map renderPair kvs) <> "}"
  where renderPair (k, v) = renderString k <> ":" <> render v

renderString :: String -> String
renderString s = "\"" <> mconcat (map escape (strToInts s)) <> "\""
  where
    escape c
        | c == cQuote = "\\\""
        | c == cBackslash = "\\\\"
        | c == 10 = "\\n"
        | c == 9 = "\\t"
        | c == 13 = "\\r"
        | otherwise = strChar c

joinWith :: String -> [String] -> String
joinWith _ [] = ""
joinWith _ [x] = x
joinWith sep (x : xs) = x <> sep <> joinWith sep xs

-- Path lookup: keys select object fields, indices select array elements.
data Step = Key String | Index Int

query :: [Step] -> JValue -> Maybe JValue
query [] v = Just v
query (Key k : rest) (JObject kvs) = case lookup k kvs of
    Nothing -> Nothing
    Just v -> query rest v
query (Index i : rest) (JArray vs)
    | i >= 0 && i < length vs = query rest (vs !! i)
    | otherwise = Nothing
query _ _ = Nothing

depth :: JValue -> Int
depth (JArray vs) = 1 + maximum (0 : map depth vs)
depth (JObject kvs) = 1 + maximum (0 : map (depth . snd) kvs)
depth _ = 0

documents :: [String]
documents =
    [ "null"
    , "  [1, 2.5, -3, 1.5e2, 0.001, true, false, null]  "
    , "{\"name\": \"mata\\tll\", \"tags\": [\"a\", \"b\\\"c\"], \"nested\": {\"deep\": [[[]]], \"n\": -0.5}}"
    , "\"esc\\\\aped \\/ slash\""
    , "[1, 2"
    , "{\"k\" 1}"
    , "tru"
    , "[1] extra"
    , "\"open"
    , "[-]"
    ]

report :: String -> IO ()
report doc = case parseJson doc of
    Left e -> putStrLn ("error: " <> e)
    Right v -> do
        putStrLn ("ast:   " <> show v)
        putStrLn ("json:  " <> render v)
        putStrLn ("depth: " <> show (depth v))
        putStrLn ("roundtrip: " <> show (parseJson (render v) == Right v))

main :: IO ()
main = do
    mapM_ report documents
    let doc = documents !! 2
    case parseJson doc of
        Left e -> putStrLn e
        Right v -> do
            print (query [Key "tags", Index 1] v)
            print (query [Key "nested", Key "n"] v)
            print (query [Key "nested", Key "deep", Index 0, Index 0] v)
            print (query [Key "missing"] v)
            print (query [Key "tags", Index 5] v)
            putStrLn (render (fromMaybe JNull (query [Key "nested"] v)))
