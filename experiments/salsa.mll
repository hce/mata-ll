-- Salsa20 stream cipher (DJB's original)
-- Reference: https://cr.yp.to/snuffle/spec.pdf

import Data.List (drop, replicate, take)

-- Bitwise FFI
xorB :: Int -> Int -> LuaPure "__mll_bxor" Int
bandB :: Int -> Int -> LuaPure "__mll_band" Int
borB :: Int -> Int -> LuaPure "__mll_bor" Int
shlB :: Int -> Int -> LuaPure "__mll_shl" Int
shrB :: Int -> Int -> LuaPure "__mll_shr" Int

-- String FFI
strByte :: String -> Int -> LuaPure "string.byte" Int
strLen :: String -> LuaPure "string.len" Int
strChar :: Int -> LuaPure "string.char" String

-- 32-bit mask
u32 :: Int -> Int
u32 x = bandB x 4294967295

-- Addition mod 2^32
add32 :: Int -> Int -> Int
add32 a b = bandB (a + b) 4294967295

-- Rotate left (32-bit)
rotL :: Int -> Int -> Int
rotL x n = let m = u32 x in bandB (borB (shlB m n) (shrB m (32 - n))) 4294967295

-- List index (manual, since !! is not built-in)
idx :: [Int] -> Int -> Int
idx (x:_)  0 = x
idx (_:xs) n = idx xs (n - 1)
idx _      _ = 0

-- List update: set element at index i to value v
listSet :: [Int] -> Int -> Int -> [Int]
listSet (_:xs) 0 v = v : xs
listSet (x:xs) n v = x : listSet xs (n - 1) v
listSet xs     _ _ = xs

-- Quarter round: Salsa20 column/row operation
-- qr(a, b, c, d) modifies state at indices a, b, c, d
quarterRound :: [Int] -> Int -> Int -> Int -> Int -> [Int]
quarterRound s a b c d = s4
  where
    sa = idx s a
    sb = idx s b
    sc = idx s c
    sd = idx s d
    sb' = xorB sb (rotL (add32 sa sd) 7)
    sc' = xorB sc (rotL (add32 sb' sa) 9)
    sd' = xorB sd (rotL (add32 sc' sb') 13)
    sa' = xorB sa (rotL (add32 sd' sc') 18)
    s1 = listSet s  a sa'
    s2 = listSet s1 b sb'
    s3 = listSet s2 c sc'
    s4 = listSet s3 d sd'

-- Column round: apply quarter rounds to columns
columnRound :: [Int] -> [Int]
columnRound s = s4
  where
    s1 = quarterRound s  0 4 8  12
    s2 = quarterRound s1 5 9 13 1
    s3 = quarterRound s2 10 14 2 6
    s4 = quarterRound s3 15 3 7 11

-- Row round: apply quarter rounds to rows
rowRound :: [Int] -> [Int]
rowRound s = s4
  where
    s1 = quarterRound s  0 1 2  3
    s2 = quarterRound s1 5 6 7  4
    s3 = quarterRound s2 10 11 8 9
    s4 = quarterRound s3 15 12 13 14

-- Double round = column round then row round
doubleRound :: [Int] -> [Int]
doubleRound = rowRound . columnRound

-- Apply n double rounds
applyRounds :: Int -> [Int] -> [Int]
applyRounds 0 s = s
applyRounds n s = applyRounds (n - 1) (doubleRound s)

-- Salsa20 core: 20 rounds (10 double rounds), then add original state
salsa20Core :: [Int] -> [Int]
salsa20Core input = zipWith add32 input (applyRounds 10 input)

-- Little-endian: 4 bytes -> 32-bit word
littleEndian :: Int -> Int -> Int -> Int -> Int
littleEndian b0 b1 b2 b3 = borB (borB b0 (shlB b1 8)) (borB (shlB b2 16) (shlB b3 24))

-- Load 16 little-endian words from 64 bytes
loadWords :: [Int] -> [Int]
loadWords bs = lwGo bs 0
  where
    lwGo _ 16 = []
    lwGo (b0:b1:b2:b3:rest) n = littleEndian b0 b1 b2 b3 : lwGo rest (n + 1)
    lwGo _ _ = []

-- Store 32-bit word as 4 little-endian bytes
storeWord :: Int -> [Int]
storeWord w = [bandB w 255, bandB (shrB w 8) 255, bandB (shrB w 16) 255, bandB (shrB w 24) 255]

-- Store 16 words as 64 bytes
storeWords :: [Int] -> [Int]
storeWords [] = []
storeWords (w:ws) = storeWord w ++ storeWords ws

-- Salsa20 hash: 64 bytes -> 64 bytes
salsa20Hash :: [Int] -> [Int]
salsa20Hash input = storeWords (salsa20Core (loadWords input))

-- "expand 32-byte k" constants (sigma)
sigma0 :: Int
sigma0 = littleEndian 101 120 112 97    -- "expa"

sigma1 :: Int
sigma1 = littleEndian 110 100 32 51     -- "nd 3"

sigma2 :: Int
sigma2 = littleEndian 50 45 98 121      -- "2-by"

sigma3 :: Int
sigma3 = littleEndian 116 101 32 107    -- "te k"

-- "expand 16-byte k" constants (tau)
tau0 :: Int
tau0 = littleEndian 101 120 112 97      -- "expa"

tau1 :: Int
tau1 = littleEndian 110 100 32 49       -- "nd 1"

tau2 :: Int
tau2 = littleEndian 54 45 98 121        -- "6-by"

tau3 :: Int
tau3 = littleEndian 116 101 32 107      -- "te k"

-- Load 4 little-endian words from 4 consecutive groups of 4 bytes
load4Words :: [Int] -> [Int]
load4Words (a:b:c:d:e:f:g:h:i:j:k:l:m:n:o:p:_) =
    [littleEndian a b c d, littleEndian e f g h,
     littleEndian i j k l, littleEndian m n o p]
load4Words _ = [0, 0, 0, 0]

-- Salsa20 expansion for 32-byte key
-- key: 32 bytes, nonce: 16 bytes (8-byte nonce + 8-byte counter)
salsa20Expand32 :: [Int] -> [Int] -> [Int]
salsa20Expand32 key nonce = salsa20Core state
  where
    k0 = load4Words (take 16 key)
    k1 = load4Words (drop 16 key)
    n  = load4Words nonce
    state = [sigma0, idx k0 0, idx k0 1, idx k0 2,
             idx k0 3, sigma1, idx n 0, idx n 1,
             idx n 2, idx n 3, sigma2, idx k1 0,
             idx k1 1, idx k1 2, idx k1 3, sigma3]

-- Salsa20 expansion for 16-byte key
salsa20Expand16 :: [Int] -> [Int] -> [Int]
salsa20Expand16 key nonce = salsa20Core state
  where
    k = load4Words key
    n = load4Words nonce
    state = [tau0, idx k 0, idx k 1, idx k 2,
             idx k 3, tau1, idx n 0, idx n 1,
             idx n 2, idx n 3, tau2, idx k 0,
             idx k 1, idx k 2, idx k 3, tau3]

-- Increment 64-bit little-endian counter in nonce bytes (bytes 8-15)
incCounter :: [Int] -> [Int]
incCounter nonce = take 8 nonce ++ incLE (drop 8 nonce)
  where
    incLE [] = []
    incLE (b:bs)
      | b == 255  = 0 : incLE bs
      | otherwise = (b + 1) : bs

-- Generate keystream: 64 bytes at a time
salsa20Keystream :: [Int] -> [Int] -> Int -> [Int]
salsa20Keystream key nonce 0 = []
salsa20Keystream key nonce n = take n' block ++ salsa20Keystream key (incCounter nonce) (n - n')
  where
    block = storeWords (salsa20Expand32 key nonce)
    n' = if n > 64 then 64 else n

-- Encrypt/decrypt (XOR with keystream)
salsa20Crypt :: [Int] -> [Int] -> [Int] -> [Int]
salsa20Crypt key nonce msg = zipWith xorB msg (salsa20Keystream key nonce (length msg))

-- ========== Hex utilities ==========

hexNibble :: Int -> Int
hexNibble n
  | n < 10    = n + 48
  | otherwise = n + 87

hexByte :: Int -> String
hexByte b = strChar (hexNibble (shrB b 4)) <> strChar (hexNibble (bandB b 15))

bytesToHex :: [Int] -> String
bytesToHex [] = ""
bytesToHex (b:bs) = hexByte b <> bytesToHex bs

hexVal :: Int -> Int
hexVal c
  | c >= 48 && c <= 57  = c - 48
  | c >= 97 && c <= 102 = c - 87
  | c >= 65 && c <= 70  = c - 55
  | otherwise            = 0

hexToBytes :: String -> [Int]
hexToBytes s = hexGo 1
  where
    len = strLen s
    hexGo i
      | i + 1 > len = []
      | otherwise    = borB (shlB (hexVal (strByte s i)) 4) (hexVal (strByte s (i + 1))) : hexGo (i + 2)

wordsToHex :: [Int] -> String
wordsToHex [] = ""
wordsToHex (w:ws) = bytesToHex (storeWord w) <> wordsToHex ws

-- ========== Test vectors ==========

main :: IO ()
main = do
    -- Test 1: Salsa20 core (DJB's spec, section 8)
    -- Input: all zeros -> output should be all zeros (fixed point)
    let zeroInput = replicate 16 0
    let zeroOut = salsa20Core zeroInput
    assert (zeroOut == replicate 16 0) "Salsa20 core: zero input"
    putStrLn "Salsa20 core zero input: PASS"

    -- Test 2: DJB's test vector for salsa20 hash function
    -- From the spec: the Salsa20 hash of a specific 64-byte input
    let tv1Input = [211, 159, 13, 115, 76, 55, 82, 183, 3, 117, 222, 37, 191, 187, 234, 136,
                    49, 237, 179, 48, 1, 106, 178, 219, 175, 199, 166, 48, 86, 16, 179, 207,
                    31, 240, 32, 63, 15, 83, 93, 161, 116, 147, 48, 113, 238, 55, 204, 36,
                    79, 201, 235, 79, 3, 81, 156, 47, 203, 26, 244, 243, 88, 118, 104, 54]
    let tv1Expect = [109, 42, 178, 168, 156, 240, 248, 238, 168, 196, 190, 203, 26, 110, 170, 154,
                     29, 29, 150, 26, 150, 30, 235, 249, 190, 163, 251, 48, 69, 144, 51, 57,
                     118, 40, 152, 157, 180, 57, 27, 94, 107, 42, 236, 35, 27, 111, 114, 114,
                     219, 236, 232, 135, 111, 155, 110, 18, 24, 232, 95, 158, 179, 19, 48, 202]
    let tv1Out = salsa20Hash tv1Input
    assert (tv1Out == tv1Expect) "Salsa20 hash: DJB spec test vector 1"
    putStrLn "Salsa20 hash test vector 1: PASS"

    -- Test 3: Second test vector
    let tv2Input = [88, 118, 104, 54, 79, 201, 235, 79, 3, 81, 156, 47, 203, 26, 244, 243,
                    191, 187, 234, 136, 211, 159, 13, 115, 76, 55, 82, 183, 3, 117, 222, 37,
                    86, 16, 179, 207, 31, 240, 32, 63, 15, 83, 93, 161, 116, 147, 48, 113,
                    238, 55, 204, 36, 49, 237, 179, 48, 1, 106, 178, 219, 175, 199, 166, 48]
    let tv2Expect = [215, 13, 129, 117, 54, 42, 181, 2, 125, 107, 210, 191, 160, 122, 23, 218,
                     83, 103, 205, 100, 103, 6, 65, 175, 254, 32, 200, 8, 169, 44, 161, 106,
                     129, 63, 85, 101, 151, 28, 153, 212, 98, 94, 19, 43, 100, 146, 41, 208,
                     65, 229, 189, 32, 140, 154, 207, 152, 195, 93, 72, 13, 239, 152, 196, 129]
    let tv2Out = salsa20Hash tv2Input
    assert (tv2Out == tv2Expect) "Salsa20 hash: DJB spec test vector 2"
    putStrLn "Salsa20 hash test vector 2: PASS"

    -- Test 4: Salsa20 expansion with 32-byte key
    -- Key: 1..16,201..216; Nonce: 101..116
    let expKey = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
                  201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216]
    let expNonce = [101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116]
    let expOut = storeWords (salsa20Expand32 expKey expNonce)
    let expExpect = [69, 37, 68, 39, 41, 15, 107, 193, 255, 139, 122, 6, 170, 233, 217, 98,
                     89, 144, 182, 106, 21, 51, 200, 65, 239, 49, 222, 34, 215, 114, 40, 126,
                     104, 197, 7, 225, 197, 153, 31, 2, 102, 78, 76, 176, 84, 245, 246, 184,
                     177, 160, 133, 130, 6, 72, 149, 119, 192, 195, 132, 236, 234, 103, 246, 74]
    assert (expOut == expExpect) "Salsa20 expand 32-byte key"
    putStrLn "Salsa20 expansion (32-byte key): PASS"

    -- Test 5: Salsa20 expansion with 16-byte key
    let exp16Key = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    let exp16Nonce = [101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116]
    let exp16Out = storeWords (salsa20Expand16 exp16Key exp16Nonce)
    let exp16Expect = [39, 173, 46, 248, 30, 200, 82, 17, 48, 67, 254, 239, 37, 18, 13, 247,
                       241, 200, 61, 144, 10, 55, 50, 185, 6, 47, 246, 253, 143, 86, 187, 225,
                       134, 85, 110, 246, 161, 163, 43, 235, 231, 94, 171, 51, 145, 214, 112, 29,
                       14, 232, 5, 16, 151, 140, 183, 141, 171, 9, 122, 181, 104, 182, 177, 193]
    assert (exp16Out == exp16Expect) "Salsa20 expand 16-byte key"
    putStrLn "Salsa20 expansion (16-byte key): PASS"

    -- Test 6: Encryption roundtrip
    let key = replicate 32 0
    let nonce = replicate 16 0
    let plaintext = [72, 101, 108, 108, 111, 44, 32, 83, 97, 108, 115, 97, 50, 48, 33, 0]
    let ciphertext = salsa20Crypt key nonce plaintext
    let decrypted = salsa20Crypt key nonce ciphertext
    assert (decrypted == plaintext) "Salsa20 encrypt/decrypt roundtrip"
    putStrLn $ "Ciphertext: " <> bytesToHex ciphertext
    putStrLn "Salsa20 roundtrip: PASS"

    putStrLn "All Salsa20 tests passed!"
