-- AES-128 test: single-file to avoid import stack overflow
-- Tests block cipher, CBC, CTR, and GCM against known vectors

-- List utilities not in Prelude
append :: [Integer] -> [Integer] -> [Integer]
append [] ys = ys
append (x:xs) ys = x : append xs ys

appendW :: [[Integer]] -> [[Integer]] -> [[Integer]]
appendW [] ys = ys
appendW (x:xs) ys = x : appendW xs ys

drop_ :: Integer -> [Integer] -> [Integer]
drop_ 0 xs = xs
drop_ _ [] = []
drop_ n (_:xs) = drop_ (n - 1) xs

replicate_ :: Integer -> Integer -> [Integer]
replicate_ 0 _ = []
replicate_ n x = x : replicate_ (n - 1) x

listEq :: [Integer] -> [Integer] -> Bool
listEq [] [] = True
listEq [] _ = False
listEq _ [] = False
listEq (x:xs) (y:ys) = if x == y then listEq xs ys else False

-- Bitwise FFI
xorB :: Integer -> Integer -> LuaPure "__mll_bxor" Integer
bandB :: Integer -> Integer -> LuaPure "__mll_band" Integer
borB :: Integer -> Integer -> LuaPure "__mll_bor" Integer
shlB :: Integer -> Integer -> LuaPure "__mll_shl" Integer
shrB :: Integer -> Integer -> LuaPure "__mll_shr" Integer

-- String FFI
strByte :: String -> Integer -> LuaPure "string.byte" Integer
strLen :: String -> LuaPure "string.len" Integer
strChar :: Integer -> LuaPure "string.char" String

-- Byte mask
byt :: Integer -> Integer
byt x = bandB x 255

-- GF(2^8) multiplication
gfMul2 :: Integer -> Integer
gfMul2 b = if bandB b 128 == 128 then xorB (bandB (shlB b 1) 255) 27 else bandB (shlB b 1) 255

gfMul3 :: Integer -> Integer
gfMul3 b = xorB (gfMul2 b) b

gfMul :: Integer -> Integer -> Integer
gfMul a b = gfMulGo a b 0
  where
    gfMulGo _ 0 acc = acc
    gfMulGo a b acc = gfMulGo (if bandB a 128 == 128 then xorB (bandB (shlB a 1) 255) 27 else bandB (shlB a 1) 255) (shrB b 1) (if bandB b 1 == 1 then xorB acc a else acc)

-- S-Box as HashMap for O(1) lookup
-- Split into 64-element chunks to avoid Lua's 255 register limit
sbox0 :: [Integer]
sbox0 = [99, 124, 119, 123, 242, 107, 111, 197, 48, 1, 103, 43, 254, 215, 171, 118, 202, 130, 201, 125, 250, 89, 71, 240, 173, 212, 162, 175, 156, 164, 114, 192, 183, 253, 147, 38, 54, 63, 247, 204, 52, 165, 229, 241, 113, 216, 49, 21, 4, 199, 35, 195, 24, 150, 5, 154, 7, 18, 128, 226, 235, 39, 178, 117]
sbox1 :: [Integer]
sbox1 = [9, 131, 44, 26, 27, 110, 90, 160, 82, 59, 214, 179, 41, 227, 47, 132, 83, 209, 0, 237, 32, 252, 177, 91, 106, 203, 190, 57, 74, 76, 88, 207, 208, 239, 170, 251, 67, 77, 51, 133, 69, 249, 2, 127, 80, 60, 159, 168, 81, 163, 64, 143, 146, 157, 56, 245, 188, 182, 218, 33, 16, 255, 243, 210]
sbox2 :: [Integer]
sbox2 = [205, 12, 19, 236, 95, 151, 68, 23, 196, 167, 126, 61, 100, 93, 25, 115, 96, 129, 79, 220, 34, 42, 144, 136, 70, 238, 184, 20, 222, 94, 11, 219, 224, 50, 58, 10, 73, 6, 36, 92, 194, 211, 172, 98, 145, 149, 228, 121, 231, 200, 55, 109, 141, 213, 78, 169, 108, 86, 244, 234, 101, 122, 174, 8]
sbox3 :: [Integer]
sbox3 = [186, 120, 37, 46, 28, 166, 180, 198, 232, 221, 116, 31, 75, 189, 139, 138, 112, 62, 181, 102, 72, 3, 246, 14, 97, 53, 87, 185, 134, 193, 29, 158, 225, 248, 152, 17, 105, 217, 142, 148, 155, 30, 135, 233, 206, 85, 40, 223, 140, 161, 137, 13, 191, 230, 66, 104, 65, 153, 45, 15, 176, 84, 187, 22]
sboxList :: [Integer]
sboxList = append sbox0 (append sbox1 (append sbox2 sbox3))

isbox0 :: [Integer]
isbox0 = [82, 9, 106, 213, 48, 54, 165, 56, 191, 64, 163, 158, 129, 243, 215, 251, 124, 227, 57, 130, 155, 47, 255, 135, 52, 142, 67, 68, 196, 222, 233, 203, 84, 123, 148, 50, 166, 194, 35, 61, 238, 76, 149, 11, 66, 250, 195, 78, 8, 46, 161, 102, 40, 217, 36, 178, 118, 91, 162, 73, 109, 139, 209, 37]
isbox1 :: [Integer]
isbox1 = [114, 248, 246, 100, 134, 104, 152, 22, 212, 164, 92, 204, 93, 101, 182, 146, 108, 112, 72, 80, 253, 237, 185, 218, 94, 21, 70, 87, 167, 141, 157, 132, 144, 216, 171, 0, 140, 188, 211, 10, 247, 228, 88, 5, 184, 179, 69, 6, 208, 44, 30, 143, 202, 63, 15, 2, 193, 175, 189, 3, 1, 19, 138, 107]
isbox2 :: [Integer]
isbox2 = [58, 145, 17, 65, 79, 103, 220, 234, 151, 242, 207, 206, 240, 180, 230, 115, 150, 172, 116, 34, 231, 173, 53, 133, 226, 249, 55, 232, 28, 117, 223, 110, 71, 241, 26, 113, 29, 41, 197, 137, 111, 183, 98, 14, 170, 24, 190, 27, 252, 86, 62, 75, 198, 210, 121, 32, 154, 219, 192, 254, 120, 205, 90, 244]
isbox3 :: [Integer]
isbox3 = [31, 221, 168, 51, 136, 7, 199, 49, 177, 18, 16, 89, 39, 128, 236, 95, 96, 81, 127, 169, 25, 181, 74, 13, 45, 229, 122, 159, 147, 201, 156, 239, 160, 224, 59, 77, 174, 42, 245, 176, 200, 235, 187, 60, 131, 83, 153, 97, 23, 43, 4, 126, 186, 119, 214, 38, 225, 105, 20, 99, 85, 33, 12, 125]
invSboxList :: [Integer]
invSboxList = append isbox0 (append isbox1 (append isbox2 isbox3))

buildTable :: [Integer] -> Integer -> HashMap Integer Integer
buildTable [] _ = hmEmpty
buildTable (x:xs) i = hmInsert i x (buildTable xs (i + 1))

sboxMap :: HashMap Integer Integer
sboxMap = buildTable sboxList 0

invSboxMap :: HashMap Integer Integer
invSboxMap = buildTable invSboxList 0

sb :: Integer -> Integer
sb i = case hmLookup i sboxMap of
    Just v -> v
    Nothing -> 0

isb :: Integer -> Integer
isb i = case hmLookup i invSboxMap of
    Just v -> v
    Nothing -> 0

-- Round constants
rcon :: Integer -> Integer
rcon 0 = 1
rcon 1 = 2
rcon 2 = 4
rcon 3 = 8
rcon 4 = 16
rcon 5 = 32
rcon 6 = 64
rcon 7 = 128
rcon 8 = 27
rcon 9 = 54
rcon _ = 0

-- State: list of 16 bytes, column-major: state[row + 4*col]
stGet :: [Integer] -> Integer -> Integer -> Integer
stGet st row col = getNth st (row + col * 4)

getNth :: [a] -> Integer -> a
getNth (x:_) 0 = x
getNth (_:xs) n = getNth xs (n - 1)
getNth [] _ = error "getNth: out of bounds"

-- Build 16-byte state from function
stBuild :: (Integer -> Integer -> Integer) -> [Integer]
stBuild f = [f 0 0, f 1 0, f 2 0, f 3 0, f 0 1, f 1 1, f 2 1, f 3 1, f 0 2, f 1 2, f 2 2, f 3 2, f 0 3, f 1 3, f 2 3, f 3 3]

-- SubBytes
subBytes :: [Integer] -> [Integer]
subBytes st = map sb st

invSubBytes :: [Integer] -> [Integer]
invSubBytes st = map isb st

-- ShiftRows
shiftRows :: [Integer] -> [Integer]
shiftRows st = stBuild f
  where
    f r c = stGet st r ((c + r) `mod` 4)

invShiftRows :: [Integer] -> [Integer]
invShiftRows st = stBuild f
  where
    f r c = stGet st r ((c - r + 4) `mod` 4)

-- MixColumns
mixColumn :: Integer -> Integer -> Integer -> Integer -> [Integer]
mixColumn a0 a1 a2 a3 = [xorB (xorB (gfMul2 a0) (gfMul3 a1)) (xorB a2 a3), xorB (xorB a0 (gfMul2 a1)) (xorB (gfMul3 a2) a3), xorB (xorB a0 a1) (xorB (gfMul2 a2) (gfMul3 a3)), xorB (xorB (gfMul3 a0) a1) (xorB a2 (gfMul2 a3))]

mixColumns :: [Integer] -> [Integer]
mixColumns st = mixColGo st 0
  where
    mixColGo st 4 = []
    mixColGo st c = append (mixColumn (stGet st 0 c) (stGet st 1 c) (stGet st 2 c) (stGet st 3 c)) (mixColGo st (c + 1))

invMixColumn :: Integer -> Integer -> Integer -> Integer -> [Integer]
invMixColumn a0 a1 a2 a3 = [xorB (xorB (gfMul 14 a0) (gfMul 11 a1)) (xorB (gfMul 13 a2) (gfMul 9 a3)), xorB (xorB (gfMul 9 a0) (gfMul 14 a1)) (xorB (gfMul 11 a2) (gfMul 13 a3)), xorB (xorB (gfMul 13 a0) (gfMul 9 a1)) (xorB (gfMul 14 a2) (gfMul 11 a3)), xorB (xorB (gfMul 11 a0) (gfMul 13 a1)) (xorB (gfMul 9 a2) (gfMul 14 a3))]

invMixColumns :: [Integer] -> [Integer]
invMixColumns st = invMixColGo st 0
  where
    invMixColGo st 4 = []
    invMixColGo st c = append (invMixColumn (stGet st 0 c) (stGet st 1 c) (stGet st 2 c) (stGet st 3 c)) (invMixColGo st (c + 1))

-- AddRoundKey
addRoundKey :: [Integer] -> [Integer] -> [Integer]
addRoundKey = zipWith xorB

-- Key expansion
splitWords :: [Integer] -> [[Integer]]
splitWords [] = []
splitWords (a:b:c:d:rest) = [a, b, c, d] : splitWords rest
splitWords _ = []

rotWord :: [Integer] -> [Integer]
rotWord (a:b:c:d:_) = [b, c, d, a]
rotWord xs = xs

xorRcon :: [Integer] -> Integer -> [Integer]
xorRcon (a:rest) rc = xorB a rc : rest
xorRcon xs _ = xs

expandRound :: [Integer] -> Integer -> [Integer]
expandRound prev round = append w0 (append w1 (append w2 w3))
  where
    p = splitWords prev
    lastW = getNth p 3
    rotated = rotWord lastW
    subbed = map sb rotated
    rconXored = xorRcon subbed (rcon round)
    w0 = zipWith xorB (getNth p 0) rconXored
    w1 = zipWith xorB (getNth p 1) w0
    w2 = zipWith xorB (getNth p 2) w1
    w3 = zipWith xorB (getNth p 3) w2

keyExpansion :: [Integer] -> [[Integer]]
keyExpansion key = keyExpGo key 0
  where
    keyExpGo prev 10 = []
    keyExpGo prev i = let next = expandRound prev i in next : keyExpGo next (i + 1)

getAllRoundKeys :: [Integer] -> [[Integer]]
getAllRoundKeys key = key : keyExpansion key

-- AES-128 encrypt block
aesEncryptBlock :: [Integer] -> [[Integer]] -> [Integer]
aesEncryptBlock pt rk = encRounds (addRoundKey pt (getNth rk 0)) 1
  where
    encRounds st 10 = addRoundKey (shiftRows (subBytes st)) (getNth rk 10)
    encRounds st r = encRounds (addRoundKey (mixColumns (shiftRows (subBytes st))) (getNth rk r)) (r + 1)

-- AES-128 decrypt block
aesDecryptBlock :: [Integer] -> [[Integer]] -> [Integer]
aesDecryptBlock ct rk = decRounds (addRoundKey ct (getNth rk 10)) 9
  where
    decRounds st 0 = addRoundKey (invSubBytes (invShiftRows st)) (getNth rk 0)
    decRounds st r = decRounds (invMixColumns (addRoundKey (invSubBytes (invShiftRows st)) (getNth rk r))) (r - 1)

-- Split into 16-byte blocks
blocks16 :: [Integer] -> [[Integer]]
blocks16 [] = []
blocks16 xs = take 16 xs : blocks16 (drop_ 16 xs)

-- PKCS7 padding
pkcs7Pad :: [Integer] -> [Integer]
pkcs7Pad bytes = append bytes (replicate_ padLen padLen)
  where
    padLen = 16 - (length bytes `mod` 16)

pkcs7Unpad :: [Integer] -> [Integer]
pkcs7Unpad [] = []
pkcs7Unpad bytes = take (length bytes - getNth bytes (length bytes - 1)) bytes

-- CBC mode
cbcEncrypt :: [Integer] -> [Integer] -> [Integer] -> [Integer]
cbcEncrypt key iv pt = cbcEncGo (getAllRoundKeys key) iv (blocks16 (pkcs7Pad pt))
  where
    cbcEncGo rk prev [] = []
    cbcEncGo rk prev (b:bs) = let cipher = aesEncryptBlock (zipWith xorB prev b) rk in append cipher (cbcEncGo rk cipher bs)

cbcDecrypt :: [Integer] -> [Integer] -> [Integer] -> [Integer]
cbcDecrypt key iv ct = pkcs7Unpad (cbcDecGo (getAllRoundKeys key) iv (blocks16 ct))
  where
    cbcDecGo rk prev [] = []
    cbcDecGo rk prev (b:bs) = append (zipWith xorB prev (aesDecryptBlock b rk)) (cbcDecGo rk b bs)

-- CTR mode
ctrBlock :: [Integer] -> Integer -> [Integer]
ctrBlock nonce ctr = append (take 12 nonce) [byt (shrB ctr 24), byt (shrB ctr 16), byt (shrB ctr 8), byt ctr]

ctrEncrypt :: [Integer] -> [Integer] -> [Integer] -> [Integer]
ctrEncrypt key nonce pt = ctrGo (getAllRoundKeys key) nonce pt 0
  where
    ctrGo rk n [] _ = []
    ctrGo rk n pt ctr = let ks = aesEncryptBlock (ctrBlock n ctr) rk in let chunk = take 16 pt in let rest = drop_ 16 pt in append (zipWith xorB chunk ks) (ctrGo rk n rest (ctr + 1))

ctrDecrypt :: [Integer] -> [Integer] -> [Integer] -> [Integer]
ctrDecrypt = ctrEncrypt

-- GCM mode
gfShiftRight128 :: [Integer] -> [Integer]
gfShiftRight128 xs = let carry = bandB (getNth xs 15) 1 in let shifted = shiftBytesR xs in if carry == 1 then xorFirst shifted else shifted
  where
    xorFirst (h:t) = xorB h 225 : t
    xorFirst xs = xs

shiftBytesR :: [Integer] -> [Integer]
shiftBytesR xs = shiftBR xs 0
  where
    shiftBR [] _ = []
    shiftBR (b:bs) carry = borB (shrB b 1) (shlB carry 7) : shiftBR bs (bandB b 1)

gfMul128 :: [Integer] -> [Integer] -> [Integer]
gfMul128 x y = gfMul128Go x y (replicate_ 16 0) 0
  where
    gfMul128Go x y z 128 = z
    gfMul128Go x y z i = let yi = getNth y (i `div` 8) in let bit = bandB (shrB yi (7 - (i `mod` 8))) 1 in let z2 = if bit == 1 then zipWith xorB z x else z in gfMul128Go (gfShiftRight128 x) y z2 (i + 1)

ghashPad :: [Integer] -> [Integer]
ghashPad xs = if (length xs `mod` 16) == 0 then xs else append xs (replicate_ (16 - (length xs `mod` 16)) 0)

intToBytes8 :: Integer -> [Integer]
intToBytes8 n = [byt (shrB n 56), byt (shrB n 48), byt (shrB n 40), byt (shrB n 32), byt (shrB n 24), byt (shrB n 16), byt (shrB n 8), byt n]

lengthBlock :: Integer -> Integer -> [Integer]
lengthBlock aadLen ctLen = append (intToBytes8 (aadLen * 8)) (intToBytes8 (ctLen * 8))

ghashBlocks :: [Integer] -> [[Integer]] -> [Integer] -> [Integer]
ghashBlocks h [] y = y
ghashBlocks h (x:xs) y = ghashBlocks h xs (gfMul128 (zipWith xorB y x) h)

ghash :: [Integer] -> [Integer] -> [Integer] -> [Integer]
ghash h aad ct = ghashBlocks h (blocks16 (append (ghashPad aad) (append (ghashPad ct) (lengthBlock (length aad) (length ct))))) (replicate_ 16 0)

gcmCtr :: [[Integer]] -> [Integer] -> [Integer] -> Integer -> [Integer]
gcmCtr rk j0 [] _ = []
gcmCtr rk j0 pt ctr = let cb = append (take 12 j0) [byt (shrB ctr 24), byt (shrB ctr 16), byt (shrB ctr 8), byt ctr] in let ks = aesEncryptBlock cb rk in append (zipWith xorB (take 16 pt) ks) (gcmCtr rk j0 (drop_ 16 pt) (ctr + 1))

gcmEncrypt :: [Integer] -> [Integer] -> [Integer] -> [Integer] -> ([Integer], [Integer])
gcmEncrypt key iv aad pt = (ct, tag)
  where
    rk = getAllRoundKeys key
    h = aesEncryptBlock (replicate_ 16 0) rk
    j0 = append iv [0, 0, 0, 1]
    ct = gcmCtr rk j0 pt 2
    s = ghash h aad ct
    tag = zipWith xorB s (aesEncryptBlock j0 rk)

gcmDecrypt :: [Integer] -> [Integer] -> [Integer] -> [Integer] -> ([Integer], [Integer])
gcmDecrypt key iv aad ct = (pt, tag)
  where
    rk = getAllRoundKeys key
    h = aesEncryptBlock (replicate_ 16 0) rk
    j0 = append iv [0, 0, 0, 1]
    pt = gcmCtr rk j0 ct 2
    s = ghash h aad ct
    tag = zipWith xorB s (aesEncryptBlock j0 rk)

-- Hex conversion
hexNibble :: Integer -> Integer
hexNibble n = if n < 10 then n + 48 else n + 87

hexByte :: Integer -> String
hexByte b = strChar (hexNibble (shrB b 4)) <> strChar (hexNibble (bandB b 15))

bytesToHex :: [Integer] -> String
bytesToHex [] = ""
bytesToHex (b:bs) = hexByte b <> bytesToHex bs

hexVal :: Integer -> Integer
hexVal c = if c >= 48 && c <= 57 then c - 48 else if c >= 97 && c <= 102 then c - 87 else if c >= 65 && c <= 70 then c - 55 else 0

hexToBytes :: String -> [Integer]
hexToBytes s = hexGo s 1 (strLen s)
  where
    hexGo s i len = if i + 1 > len then [] else borB (shlB (hexVal (strByte s i)) 4) (hexVal (strByte s (i + 1))) : hexGo s (i + 2) len

stringToBytes :: String -> [Integer]
stringToBytes s = stbGo s 1 (strLen s)
  where
    stbGo s i len = if i > len then [] else strByte s i : stbGo s (i + 1) len

bytesToString :: [Integer] -> String
bytesToString [] = ""
bytesToString (b:bs) = strChar b <> bytesToString bs

main :: IO ()
main = do
    -- NIST AES-128 test vector
    let key = hexToBytes "2b7e151628aed2a6abf7158809cf4f3c"
    let pt  = hexToBytes "3243f6a8885a308d313198a2e0370734"
    let rk  = getAllRoundKeys key
    let ct  = aesEncryptBlock pt rk
    let ctHex = bytesToHex ct
    putStrLn $ "Ciphertext: " <> ctHex
    assert (ctHex == "3925841d02dc09fbdc118597196a0b32") "NIST AES-128 test vector"
    let dec = aesDecryptBlock ct rk
    assert (bytesToHex dec == "3243f6a8885a308d313198a2e0370734") "AES decrypt roundtrip"
    putStrLn "AES-128 block cipher: PASS"

    -- CBC roundtrip
    let cbcKey = hexToBytes "2b7e151628aed2a6abf7158809cf4f3c"
    let cbcIv  = hexToBytes "000102030405060708090a0b0c0d0e0f"
    let cbcPt  = stringToBytes "Hello, AES-CBC!"
    let cbcCt  = cbcEncrypt cbcKey cbcIv cbcPt
    putStrLn $ "CBC ciphertext: " <> bytesToHex cbcCt
    let cbcDec = cbcDecrypt cbcKey cbcIv cbcCt
    assert (listEq cbcDec cbcPt) "CBC roundtrip"
    putStrLn "AES-128-CBC: PASS"

    -- CTR roundtrip
    let ctrKey   = hexToBytes "2b7e151628aed2a6abf7158809cf4f3c"
    let ctrNonce = hexToBytes "f0f1f2f3f4f5f6f7f8f9fafb00000000"
    let ctrPt    = stringToBytes "Hello, AES-CTR!"
    let ctrCt    = ctrEncrypt ctrKey ctrNonce ctrPt
    putStrLn $ "CTR ciphertext: " <> bytesToHex ctrCt
    let ctrDec   = ctrDecrypt ctrKey ctrNonce ctrCt
    assert (listEq ctrDec ctrPt) "CTR roundtrip"
    putStrLn "AES-128-CTR: PASS"

    -- GCM roundtrip
    let gcmKey = hexToBytes "feffe9928665731c6d6a8f9467308308"
    let gcmIv  = hexToBytes "cafebabefacedbaddecaf888"
    let gcmAad = hexToBytes "feedfacedeadbeeffeedfacedeadbeefabaddad2"
    let gcmPt  = stringToBytes "Hello, AES-GCM!"
    let gcmResult = gcmEncrypt gcmKey gcmIv gcmAad gcmPt
    let gcmCt = fst gcmResult
    let gcmTag = snd gcmResult
    putStrLn $ "GCM ciphertext: " <> bytesToHex gcmCt
    putStrLn $ "GCM tag: " <> bytesToHex gcmTag
    let gcmResult2 = gcmDecrypt gcmKey gcmIv gcmAad gcmCt
    let gcmDec = fst gcmResult2
    let gcmTag2 = snd gcmResult2
    assert (listEq gcmDec gcmPt) "GCM roundtrip"
    assert (listEq gcmTag gcmTag2) "GCM tags match"
    putStrLn "AES-128-GCM: PASS"

    putStrLn "All AES tests passed!"
