-- GHC cgrun064: Roman numeral conversion

data RomanPair = RomanPair Int String

romanTable :: [RomanPair]
romanTable = [RomanPair 1000 "M", RomanPair 900 "CM", RomanPair 500 "D", RomanPair 400 "CD", RomanPair 100 "C", RomanPair 90 "XC", RomanPair 50 "L", RomanPair 40 "XL", RomanPair 10 "X", RomanPair 9 "IX", RomanPair 5 "V", RomanPair 4 "IV", RomanPair 1 "I"]

toRoman :: Int -> String
toRoman 0 = ""
toRoman n = go n romanTable
  where
    go _ [] = ""
    go m (RomanPair val sym : rest)
        | m >= val  = sym <> go (m - val) (RomanPair val sym : rest)
        | otherwise = go m rest

main :: IO ()
main = do
    assert (toRoman 1 == "I") "1"
    assert (toRoman 4 == "IV") "4"
    assert (toRoman 5 == "V") "5"
    assert (toRoman 9 == "IX") "9"
    assert (toRoman 14 == "XIV") "14"
    assert (toRoman 40 == "XL") "40"
    assert (toRoman 58 == "LVIII") "58"
    assert (toRoman 90 == "XC") "90"
    assert (toRoman 400 == "CD") "400"
    assert (toRoman 900 == "CM") "900"
    assert (toRoman 1994 == "MCMXCIV") "1994"
    assert (toRoman 2024 == "MMXXIV") "2024"
    assert (toRoman 3999 == "MMMCMXCIX") "3999"
    putStrLn "ok"
