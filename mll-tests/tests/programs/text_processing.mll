-- Text utilities over code lists: run-length coding, a Caesar cipher,
-- palindromes, word reversal, greedy word wrap, Roman numerals both
-- ways, and numbers spelled out in English.
-- Leans on: LString (strByte/strSub/strLen/strChar), span/break-style
-- splitting, guards over Int ranges, integer
-- division chains, string accumulation.
import LString

joinWith :: String -> [String] -> String
joinWith _ [] = ""
joinWith _ [s] = s
joinWith sep (s : ss) = s <> sep <> joinWith sep ss

fromCodes :: [Int] -> String
fromCodes cs = mconcat (map strChar cs)

isLetter :: Int -> Bool
isLetter c = (c >= 65 && c <= 90) || (c >= 97 && c <= 122)

isUpper :: Int -> Bool
isUpper c = c >= 65 && c <= 90

toLower :: Int -> Int
toLower c = if isUpper c then c + 32 else c

-- Run-length encoding as (count, code) pairs and its rendering.
rle :: String -> [(Int, Int)]
rle s = go (strToInts s)
  where
    go [] = []
    go (c : cs) = let (same, rest) = span (== c) cs in (1 + length same, c) : go rest

renderRle :: [(Int, Int)] -> String
renderRle = mconcat . map (\(n, c) -> show n <> strChar c)

unRle :: [(Int, Int)] -> String
unRle = mconcat . map (\(n, c) -> mconcat (replicate n (strChar c)))

caesar :: Int -> String -> String
caesar k s = fromCodes (map shift (strToInts s))
  where
    shift c
        | isUpper c = 65 + (c - 65 + k) `mod` 26
        | c >= 97 && c <= 122 = 97 + (c - 97 + k) `mod` 26
        | otherwise = c

isPalindrome :: String -> Bool
isPalindrome s = cleaned == reverse cleaned
  where cleaned = map toLower (filter isLetter (strToInts s))

wordsOf :: String -> [String]
wordsOf s = go (strToInts s)
  where
    go cs = case span (/= 32) (dropWhile (== 32) cs) of
        ([], _) -> []
        (w, rest) -> fromCodes w : go rest

reverseWords :: String -> String
reverseWords = joinWith " " . reverse . wordsOf

reverseEachWord :: String -> String
reverseEachWord = joinWith " " . map (fromCodes . reverse . strToInts) . wordsOf

-- Greedy wrap: fill each line up to the width, never splitting a word.
wrap :: Int -> String -> [String]
wrap width s = go (wordsOf s) [] 0
  where
    go [] line _ = [joinWith " " (reverse line) | not (null line)]
    go (w : ws) line used
        | null line = go ws [w] (strLen w)
        | used + 1 + strLen w <= width = go ws (w : line) (used + 1 + strLen w)
        | otherwise = joinWith " " (reverse line) : go (w : ws) [] 0

toRoman :: Int -> String
toRoman 0 = ""
toRoman n = go n numerals
  where
    numerals = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")]
    go 0 _ = ""
    go _ [] = ""
    go k ((v, sym) : rest)
        | k >= v = sym <> go (k - v) ((v, sym) : rest)
        | otherwise = go k rest

fromRoman :: String -> Int
fromRoman s = go (map value (strToInts s))
  where
    value c
        | c == 73 = 1
        | c == 86 = 5
        | c == 88 = 10
        | c == 76 = 50
        | c == 67 = 100
        | c == 68 = 500
        | c == 77 = 1000
        | otherwise = 0
    go [] = 0
    go [x] = x
    go (x : y : rest)
        | x < y = y - x + go rest
        | otherwise = x + go (y : rest)

spell :: Int -> String
spell n
    | n < 0 = "minus " <> spell (negate n)
    | n < 20 = small !! n
    | n < 100 = tens !! (n `div` 10) <> (if n `mod` 10 == 0 then "" else "-" <> small !! (n `mod` 10))
    | n < 1000 = small !! (n `div` 100) <> " hundred" <> rest (n `mod` 100)
    | n < 1000000 = spell (n `div` 1000) <> " thousand" <> rest (n `mod` 1000)
    | otherwise = spell (n `div` 1000000) <> " million" <> rest (n `mod` 1000000)
  where
    small = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen"]
    tens = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"]
    rest 0 = ""
    rest k = " " <> spell k

capitalize :: String -> String
capitalize s = case strToInts s of
    [] -> ""
    (c : cs) -> strChar (if c >= 97 && c <= 122 then c - 32 else c) <> fromCodes cs

main :: IO ()
main = do
    let sample = "aaabccddddde"
    print (rle sample)
    putStrLn (renderRle (rle sample))
    print (unRle (rle sample) == sample, unRle (rle "") == "")
    putStrLn (caesar 3 "Hello, World! xyz")
    putStrLn (caesar (-3) (caesar 3 "Hello, World! xyz"))
    putStrLn (caesar 13 (caesar 13 "round trip"))
    print (map isPalindrome ["A man, a plan, a canal: Panama", "hello", "", "Was it a car or a cat I saw"])
    putStrLn (reverseWords "the quick  brown   fox")
    putStrLn (reverseEachWord "the quick brown fox")
    mapM_ putStrLn (wrap 20 "The quick brown fox jumps over the lazy dog and keeps running far away")
    print (wrap 5 "", wrap 3 "abcdefgh ij")
    print (map toRoman [1, 4, 9, 14, 40, 90, 400, 1994, 2024, 3999])
    print (map fromRoman ["I", "IV", "IX", "XIV", "XL", "XC", "CD", "MCMXCIV", "MMXXIV", "MMMCMXCIX"])
    print (all (\n -> fromRoman (toRoman n) == n) [1 .. 500])
    mapM_ (putStrLn . spell) [0, 7, 13, 20, 42, 100, 101, 999, 1000, 12345, 1000000, 987654321, -15]
    putStrLn (capitalize "hello" <> " " <> capitalize "" <> capitalize "World")
    print (strSub "hello world" 1 5, strSub "hello world" (-5) (-1), strSub "hello" 4 100, strSub "hello" 3 2)
    print (strByte "A" 1, strByte "abc" (-1), strLen "", strLen "four")
