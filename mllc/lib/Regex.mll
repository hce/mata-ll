import LString (strByte, strLen, strSub)

-- Regular expression engine for MLL
-- CPS-based backtracking matcher with precompiled pattern AST
-- Supports: . * + ? | () [] [^] ^ $ \d \w \s \D \W \S \n \t \r \\

-- Regex AST
data RE = RLit Integer | RDot | RSeq RE RE | RAlt RE RE | RStar RE | RPlus RE | ROpt RE | RClass [CItem] | RNClass [CItem] | RAnchorS | RAnchorE | REmpty

data CItem = CChar Integer | CRange Integer Integer

data Match = Match Integer Integer

data PResult = POk RE Integer | PErr String

-- Helpers

seqRE :: RE -> RE -> RE
seqRE REmpty b = b
seqRE a b = RSeq a b

ccDigit :: [CItem]
ccDigit = CRange 48 57 : []

ccWord :: [CItem]
ccWord = CRange 97 122 : CRange 65 90 : CRange 48 57 : CChar 95 : []

ccSpace :: [CItem]
ccSpace = CChar 32 : CChar 9 : CChar 10 : CChar 13 : []

matchItem :: CItem -> Integer -> Bool
matchItem (CChar x) c = x == c
matchItem (CRange lo hi) c = c >= lo && c <= hi

matchAny :: [CItem] -> Integer -> Bool
matchAny [] _ = False
matchAny (item:rest) c = matchItem item c || matchAny rest c

-- Matcher: CPS-based backtracking

matchAt :: RE -> String -> Integer -> Integer -> (Integer -> Maybe Integer) -> Maybe Integer
matchAt re s i len k = case re of
    REmpty -> k i
    RLit c -> if i <= len && strByte s i == c then k (i + 1) else Nothing
    RDot -> if i <= len && strByte s i /= 10 then k (i + 1) else Nothing
    RAnchorS -> if i == 1 then k i else Nothing
    RAnchorE -> if i > len then k i else Nothing
    RSeq a b -> matchAt a s i len (\j -> matchAt b s j len k)
    RAlt a b -> matchAlt a b s i len k
    RStar a -> matchStar a s i len k
    RPlus a -> matchAt a s i len (\j -> if j == i then k j else matchStar a s j len k)
    ROpt a -> matchOpt a s i len k
    RClass items -> if i <= len && matchAny items (strByte s i) then k (i + 1) else Nothing
    RNClass items -> if i <= len && strByte s i /= 10 && not (matchAny items (strByte s i)) then k (i + 1) else Nothing

matchAlt :: RE -> RE -> String -> Integer -> Integer -> (Integer -> Maybe Integer) -> Maybe Integer
matchAlt a b s i len k = case matchAt a s i len k of
    Just r -> Just r
    Nothing -> matchAt b s i len k

matchOpt :: RE -> String -> Integer -> Integer -> (Integer -> Maybe Integer) -> Maybe Integer
matchOpt a s i len k = case matchAt a s i len k of
    Just r -> Just r
    Nothing -> k i

matchStar :: RE -> String -> Integer -> Integer -> (Integer -> Maybe Integer) -> Maybe Integer
matchStar a s i len k = case matchAt a s i len (\j -> if j == i then k j else matchStar a s j len k) of
    Just r -> Just r
    Nothing -> k i

-- Parser: pattern string -> RE

compile :: String -> Either String RE
compile pat = compileLen pat (strLen pat)

compileLen :: String -> Integer -> Either String RE
compileLen pat 0 = Right REmpty
compileLen pat len = finishCompile len (parseAlt pat 1 len)

finishCompile :: Integer -> PResult -> Either String RE
finishCompile len (PErr e) = Left e
finishCompile len (POk re pos) = if pos > len then Right re else Left ("Unexpected character at position " <> show pos)

parseAlt :: String -> Integer -> Integer -> PResult
parseAlt pat pos len = altDone pat len (parseSeq pat pos len)

altDone :: String -> Integer -> PResult -> PResult
altDone pat len (PErr e) = PErr e
altDone pat len (POk left pos2) = if pos2 <= len && strByte pat pos2 == 124 then altRight left (parseAlt pat (pos2 + 1) len) else POk left pos2

altRight :: RE -> PResult -> PResult
altRight left (PErr e) = PErr e
altRight left (POk right pos3) = POk (RAlt left right) pos3

parseSeq :: String -> Integer -> Integer -> PResult
parseSeq pat pos len = parseSeqAcc pat pos len REmpty

parseSeqAcc :: String -> Integer -> Integer -> RE -> PResult
parseSeqAcc pat pos len acc = if pos > len then POk acc pos else seqCheck pat pos len acc (strByte pat pos)

seqCheck :: String -> Integer -> Integer -> RE -> Integer -> PResult
seqCheck pat pos len acc 124 = POk acc pos
seqCheck pat pos len acc 41 = POk acc pos
seqCheck pat pos len acc _ = seqNext pat pos len acc (parseQuantified pat pos len)

seqNext :: String -> Integer -> Integer -> RE -> PResult -> PResult
seqNext pat pos len acc (PErr e) = PErr e
seqNext pat pos len acc (POk re pos2) = parseSeqAcc pat pos2 len (seqRE acc re)

parseQuantified :: String -> Integer -> Integer -> PResult
parseQuantified pat pos len = quantDone pat len (parseAtom pat pos len)

quantDone :: String -> Integer -> PResult -> PResult
quantDone pat len (PErr e) = PErr e
quantDone pat len (POk re pos2) = if pos2 > len then POk re pos2 else applyQ re (strByte pat pos2) pos2

applyQ :: RE -> Integer -> Integer -> PResult
applyQ re 42 pos = POk (RStar re) (pos + 1)
applyQ re 43 pos = POk (RPlus re) (pos + 1)
applyQ re 63 pos = POk (ROpt re) (pos + 1)
applyQ re _ pos = POk re pos

parseAtom :: String -> Integer -> Integer -> PResult
parseAtom pat pos len = if pos > len then PErr "Unexpected end of pattern" else atomByte pat pos len (strByte pat pos)

atomByte :: String -> Integer -> Integer -> Integer -> PResult
atomByte pat pos len 46 = POk RDot (pos + 1)
atomByte pat pos len 94 = POk RAnchorS (pos + 1)
atomByte pat pos len 36 = POk RAnchorE (pos + 1)
atomByte pat pos len 40 = parseGroup pat (pos + 1) len
atomByte pat pos len 91 = parseCharClass pat (pos + 1) len
atomByte pat pos len 92 = parseEscape pat (pos + 1) len
atomByte pat pos len c = POk (RLit c) (pos + 1)

parseGroup :: String -> Integer -> Integer -> PResult
parseGroup pat pos len = groupDone pat pos len (parseAlt pat pos len)

groupDone :: String -> Integer -> Integer -> PResult -> PResult
groupDone pat pos len (PErr e) = PErr e
groupDone pat pos len (POk re pos2) = if pos2 <= len && strByte pat pos2 == 41 then POk re (pos2 + 1) else PErr "Missing closing parenthesis"

parseEscape :: String -> Integer -> Integer -> PResult
parseEscape pat pos len = if pos > len then PErr "Unexpected end after \\" else escByte (strByte pat pos) pos

escByte :: Integer -> Integer -> PResult
escByte 100 pos = POk (RClass ccDigit) (pos + 1)
escByte 68 pos = POk (RNClass ccDigit) (pos + 1)
escByte 119 pos = POk (RClass ccWord) (pos + 1)
escByte 87 pos = POk (RNClass ccWord) (pos + 1)
escByte 115 pos = POk (RClass ccSpace) (pos + 1)
escByte 83 pos = POk (RNClass ccSpace) (pos + 1)
escByte 110 pos = POk (RLit 10) (pos + 1)
escByte 116 pos = POk (RLit 9) (pos + 1)
escByte 114 pos = POk (RLit 13) (pos + 1)
escByte c pos = POk (RLit c) (pos + 1)

parseCharClass :: String -> Integer -> Integer -> PResult
parseCharClass pat pos len = if pos > len then PErr "Unterminated character class" else if strByte pat pos == 94 then classBody pat (pos + 1) len True [] else classBody pat pos len False []

classBody :: String -> Integer -> Integer -> Bool -> [CItem] -> PResult
classBody pat pos len neg acc = if pos > len then PErr "Unterminated character class" else classByte pat pos len neg acc (strByte pat pos)

classByte :: String -> Integer -> Integer -> Bool -> [CItem] -> Integer -> PResult
classByte pat pos len neg acc 93 = if neg then POk (RNClass acc) (pos + 1) else POk (RClass acc) (pos + 1)
classByte pat pos len neg acc 92 = classEsc pat (pos + 1) len neg acc
classByte pat pos len neg acc c = if pos + 2 <= len && strByte pat (pos + 1) == 45 && strByte pat (pos + 2) /= 93 then classBody pat (pos + 3) len neg (CRange c (strByte pat (pos + 2)) : acc) else classBody pat (pos + 1) len neg (CChar c : acc)

classEsc :: String -> Integer -> Integer -> Bool -> [CItem] -> PResult
classEsc pat pos len neg acc = if pos > len then PErr "Unterminated escape in class" else classEscByte pat pos len neg acc (strByte pat pos)

classEscByte :: String -> Integer -> Integer -> Bool -> [CItem] -> Integer -> PResult
classEscByte pat pos len neg acc 100 = classBody pat (pos + 1) len neg (CRange 48 57 : acc)
classEscByte pat pos len neg acc 119 = classBody pat (pos + 1) len neg (CRange 97 122 : CRange 65 90 : CRange 48 57 : CChar 95 : acc)
classEscByte pat pos len neg acc 115 = classBody pat (pos + 1) len neg (CChar 32 : CChar 9 : CChar 10 : CChar 13 : acc)
classEscByte pat pos len neg acc c = classBody pat (pos + 1) len neg (CChar c : acc)

-- Public API

test :: RE -> String -> Bool
test re s = case find re s of
    Just _ -> True
    Nothing -> False

find :: RE -> String -> Maybe Match
find re s = findFrom re s 1 (strLen s)

findFrom :: RE -> String -> Integer -> Integer -> Maybe Match
findFrom re s i len = if i > len + 1 then Nothing else tryMatch re s i len (matchAt re s i len (\j -> Just j))

tryMatch :: RE -> String -> Integer -> Integer -> Maybe Integer -> Maybe Match
tryMatch re s i len (Just end) = Just (Match i (end - i))
tryMatch re s i len Nothing = findFrom re s (i + 1) len

findStr :: RE -> String -> Maybe String
findStr re s = case find re s of
    Just (Match start matchLen) -> Just (strSub s start (start + matchLen - 1))
    Nothing -> Nothing

matchFull :: RE -> String -> Bool
matchFull re s = matchFullLen re s (strLen s)

matchFullLen :: RE -> String -> Integer -> Bool
matchFullLen re s len = case matchAt re s 1 len (\j -> if j > len then Just j else Nothing) of
    Just _ -> True
    Nothing -> False
