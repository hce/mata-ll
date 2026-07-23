import LBit (band, bor, shiftL, shiftR)

strByte2 :: String -> Int -> LuaPure "string.byte" Int
strLen2 :: String -> LuaPure "string.len" Int
strChar2 :: Int -> LuaPure "string.char" String

hexNibble :: Int -> Int
hexNibble n
  | n < 10    = n + 48
  | otherwise = n + 87

hexByte :: Int -> String
hexByte b = strChar2 (hexNibble (shiftR b 4)) <> strChar2 (hexNibble (band b 15))

bytesToHex :: [Int] -> String
bytesToHex [] = ""
bytesToHex (b:bs) = hexByte b <> bytesToHex bs

bsToHex :: ByteString -> String
bsToHex bs = bytesToHex (bsUnpack bs)

hexVal :: Int -> Int
hexVal c
  | c >= 48 && c <= 57  = c - 48
  | c >= 97 && c <= 102 = c - 87
  | c >= 65 && c <= 70  = c - 55
  | otherwise            = 0

hexToBytes :: String -> [Int]
hexToBytes s = hexGo 1
  where
    len = strLen2 s
    hexGo i
      | i + 1 > len = []
      | otherwise    = bor (shiftL (hexVal (strByte2 s i)) 4) (hexVal (strByte2 s (i + 1))) : hexGo (i + 2)

hexToBs :: String -> ByteString
hexToBs s = bsPack (hexToBytes s)


