import LString (strByte, strLen, strSub, strChar)

-- JSON value type
data Json = JNull | JBool Bool | JNum Number | JStr String | JArr [Json] | JObj [(String, Json)]

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
        JOk val pos2 -> Right val

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
-- ================================================================

parseNumber :: String -> Integer -> Integer -> JResult
parseNumber s pos len = scanNumber s pos len pos False

scanNumber :: String -> Integer -> Integer -> Integer -> Bool -> JResult
scanNumber s start len pos hasDot = if pos > len then finishNumber s start pos hasDot else scanDigit s start len pos hasDot (strByte s pos)

scanDigit :: String -> Integer -> Integer -> Integer -> Bool -> Integer -> JResult
scanDigit s start len pos hasDot 45 = if pos == start then scanNumber s start len (pos + 1) hasDot else finishNumber s start pos hasDot
scanDigit s start len pos hasDot 43 = scanNumber s start len (pos + 1) hasDot
scanDigit s start len pos hasDot 46 = if hasDot then finishNumber s start pos hasDot else scanNumber s start len (pos + 1) True
scanDigit s start len pos hasDot 101 = scanNumber s start len (pos + 1) True
scanDigit s start len pos hasDot 69 = scanNumber s start len (pos + 1) True
scanDigit s start len pos hasDot c = if c >= 48 && c <= 57 then scanNumber s start len (pos + 1) hasDot else finishNumber s start pos hasDot

finishNumber :: String -> Integer -> Integer -> Bool -> JResult
finishNumber s start pos hasDot = if pos == start then JErr "Expected number" else JOk (JNum (toNumber (strSub s start (pos - 1)))) (skipWS s pos (strLen s))

toNumber :: String -> LuaPure "tonumber" Number

-- ================================================================
-- Strings
-- ================================================================

parseStringVal :: String -> Integer -> Integer -> JResult
parseStringVal s pos len = case parseStr s (pos + 1) len of
    SErr e -> JErr e
    SOk str pos2 -> JOk (JStr str) pos2

parseStr :: String -> Integer -> Integer -> SResult
parseStr s pos len = scanStr s pos len pos

scanStr :: String -> Integer -> Integer -> Integer -> SResult
scanStr s start len pos = if pos > len then SErr "Unterminated string" else scanStrByte s start len pos (strByte s pos)

scanStrByte :: String -> Integer -> Integer -> Integer -> Integer -> SResult
scanStrByte s start len pos 34 = SOk (strSub s start (pos - 1)) (skipWS s (pos + 1) len)
scanStrByte s start len pos 92 = if pos + 1 <= len then scanStr s start len (pos + 2) else SErr "Unterminated escape"
scanStrByte s start len pos _ = scanStr s start len (pos + 1)

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
