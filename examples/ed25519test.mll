-- Ed25519 test suite
-- Tests SHA-512, field arithmetic, and Ed25519 sign/verify against RFC 8032 vectors

import Crypto.Ed25519

strByte2 :: String -> Integer -> LuaPure "string.byte" Integer
strLen2 :: String -> LuaPure "string.len" Integer
strChar2 :: Integer -> LuaPure "string.char" String

-- Hex utilities
hexNibble :: Integer -> Integer
hexNibble n
  | n < 10    = n + 48
  | otherwise = n + 87

hexByte :: Integer -> String
hexByte b = strChar2 (hexNibble (shrB b 4)) <> strChar2 (hexNibble (bandB b 15))

bytesToHex :: [Integer] -> String
bytesToHex [] = ""
bytesToHex (b:bs) = hexByte b <> bytesToHex bs

bsToHex :: ByteString -> String
bsToHex bs = bytesToHex (bsUnpack bs)

hexVal :: Integer -> Integer
hexVal c
  | c >= 48 && c <= 57  = c - 48
  | c >= 97 && c <= 102 = c - 87
  | c >= 65 && c <= 70  = c - 55
  | otherwise            = 0

hexToBytes :: String -> [Integer]
hexToBytes s = hexGo 1
  where
    len = strLen2 s
    hexGo i
      | i + 1 > len = []
      | otherwise    = borB (shlB (hexVal (strByte2 s i)) 4) (hexVal (strByte2 s (i + 1))) : hexGo (i + 2)

hexToBs :: String -> ByteString
hexToBs s = bsPack (hexToBytes s)

main :: IO ()
main = do
    -- ============================================================
    -- Test SHA-512 against known vectors
    -- ============================================================

    -- SHA-512("") = cf83e1357eefb8bd...
    let emptyHash = bsToHex (sha512 bsEmpty)
    assert (emptyHash == "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e") "SHA-512 empty"
    putStrLn "SHA-512 empty: PASS"

    -- SHA-512("abc") = ddaf35a193617aba...
    let abcHash = bsToHex (sha512 (bsPack [97, 98, 99]))
    assert (abcHash == "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f") "SHA-512 abc"
    putStrLn "SHA-512 abc: PASS"

    -- ============================================================
    -- Test Ed25519 key generation (RFC 8032 Test Vector 1)
    -- ============================================================

    let seed1 = hexToBs "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"
    let (pub1, sec1) = ed25519Keypair seed1
    let pub1Hex = bsToHex pub1
    assert (pub1Hex == "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a") "keypair 1 public key"
    putStrLn "Keypair 1: PASS"

    -- ============================================================
    -- Test Ed25519 signing (RFC 8032 Test Vector 1: empty message)
    -- ============================================================

    let sig1 = ed25519Sign sec1 pub1 bsEmpty
    let sig1Hex = bsToHex sig1
    assert (sig1Hex == "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b") "sign 1"
    putStrLn "Sign 1 (empty msg): PASS"

    -- ============================================================
    -- Test Ed25519 verification
    -- ============================================================

    assert (ed25519Verify pub1 bsEmpty sig1) "verify 1"
    putStrLn "Verify 1: PASS"

    -- Reject tampered signature
    let badSig = bsConcat (bsSub sig1 0 31) (bsPack [0]) `bsConcat` bsSub sig1 32 32
    assert (not (ed25519Verify pub1 bsEmpty badSig)) "verify 1 tampered"
    putStrLn "Verify 1 tampered: PASS"

    -- ============================================================
    -- RFC 8032 Test Vector 2: 1-byte message (0x72)
    -- ============================================================

    let seed2 = hexToBs "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb"
    let (pub2, sec2) = ed25519Keypair seed2
    assert (bsToHex pub2 == "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c") "keypair 2"
    putStrLn "Keypair 2: PASS"

    let msg2 = bsPack [114]
    let sig2 = ed25519Sign sec2 pub2 msg2
    assert (bsToHex sig2 == "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00") "sign 2"
    putStrLn "Sign 2: PASS"

    assert (ed25519Verify pub2 msg2 sig2) "verify 2"
    putStrLn "Verify 2: PASS"

    putStrLn "All Ed25519 tests passed!"
