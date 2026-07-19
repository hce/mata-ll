-- lua-compat-skip: luajit
--   (a getU64 whose bytes set the top bit is a value > 2^53; LuaJIT's doubles
--   cannot represent it, so the read and the masking below round before they
--   run — a documented limitation, see doc/articles/CAVEATS.md. The contract
--   holds on 64-bit-integer hosts.)
-- 64-bit ByteString reads whose bytes set the top bit (a getU64 producing a
-- Lua-negative), plus the downstream masking the zpool reader does on them.
--
-- bytestring.mll already covers bsGetI8 (negative) and bsGetU16LE, but nothing
-- reads a full 64-bit value with bit 63 set — which is the reader's actual hot
-- path: block-pointer words and the ZAP block-type magics are 64-bit values
-- whose high bit is set, decoded by composing the narrower intrinsics with
-- LBit shifts/ors, then masked. This pins that composition end to end.
--
-- CONTRACT: ByteString is backed by an immutable Lua string; the getU*LE
-- intrinsics return native Lua 5.4 integers. A 64-bit value >= 2^63 therefore
-- comes back as a NEGATIVE integer (the top-bit-set bit pattern), and all
-- consumers mask/shift it rather than compare magnitudes. LBit shiftR is
-- logical (see lbit_64bit_boundary.mll). Both getU64BE (built from bsIndex, as
-- ZBytes.getU64BE is) and getU64LE (built from the bsGetU32LE intrinsic, as
-- ZBytes.getU64LE is) must agree on the same on-disk bytes.
--
-- Expected values were computed from the Lua reference (string.unpack ">i8"/
-- "<i8"): the byte pattern 80 00 00 00 00 00 00 03 decodes to
-- 0x8000000000000003 = -9223372036854775805 as a signed 64-bit integer.

import LBit (band, bor, shiftL, shiftR)

-- 2^63 - 1 and -2^63, written in corpus style (2^63 overflows a literal).
maxI63 :: Integer
maxI63 = 9223372036854775807

minI63 :: Integer
minI63 = 0 - 9223372036854775807 - 1

-- Big-endian readers, mirroring ZBytes exactly (bsIndex-based).
getU16BE :: ByteString -> Integer -> Integer
getU16BE b off = bsIndex b off * 256 + bsIndex b (off + 1)

getU32BE :: ByteString -> Integer -> Integer
getU32BE b off = shiftL (getU16BE b off) 16 + getU16BE b (off + 2)

getU64BE :: ByteString -> Integer -> Integer
getU64BE b off = bor (shiftL (getU32BE b off) 32) (getU32BE b (off + 4))

-- Little-endian 64-bit read, built from the bsGetU32LE intrinsic (as ZBytes
-- getU64LE is): high dword shifted up (into the sign bit) or'd with the low.
getU64LE :: ByteString -> Integer -> Integer
getU64LE b off = bor (bsGetU32LE b off) (shiftL (bsGetU32LE b (off + 4)) 32)

main :: IO ()
main = do
    -- Big-endian bytes 80 00 00 00 00 00 00 03 -> 0x8000000000000003.
    let be = bsPack [128, 0, 0, 0, 0, 0, 0, 3]
    -- Little-endian bytes for the SAME 64-bit value 0x8000000000000003.
    let le = bsPack [3, 0, 0, 0, 0, 0, 0, 128]

    -- The 64-bit read comes back NEGATIVE (top bit set); both orders agree.
    assert (getU64BE be 0 == minI63 + 3) "getU64BE of 0x80..03 is negative (-2^63+3)"
    assert (getU64LE le 0 == minI63 + 3) "getU64LE of 0x80..03 is negative (-2^63+3)"
    assert (getU64BE be 0 == getU64LE le 0) "BE and LE reads of the same value agree"

    -- A high dword whose top bit is set, lifted into bit 63 via shiftL 32,
    -- must land in the sign bit — the exact step block-pointer decode relies on.
    assert (bsGetU32LE le 4 == 2147483648) "high dword reads as 0x80000000 (positive 32-bit)"
    assert (shiftL (bsGetU32LE le 4) 32 == minI63) "high dword << 32 lands in the sign bit"

    -- Downstream masking of the sign-bit-set value (as the reader strips the
    -- ZAP block-type high bits, and decodes ZPL directory entries).
    assert (band (getU64BE be 0) (shiftL 1 63 - 1) == 3) "mask low 63 bits recovers 3"
    assert (band (getU64BE be 0) 281474976710655 == 3) "mask low 48 bits (object id) == 3"
    assert (band (shiftR (getU64BE be 0) 60) 15 == 8) "top nibble (entry type) == 8"

    -- The pure sign bit on its own: bytes 80 00 00 00 00 00 00 00 -> -2^63.
    let sb = bsPack [128, 0, 0, 0, 0, 0, 0, 0]
    assert (getU64BE sb 0 == minI63) "getU64BE of 0x80..00 == -2^63 (zap-leaf magic)"
    assert (shiftR (getU64BE sb 0) 63 == 1) "logical shiftR brings the sign bit to bit 0"

    -- All bits set: bytes FF*8 -> -1; logical shiftR must not sign-extend.
    let ones = bsPack [255, 255, 255, 255, 255, 255, 255, 255]
    assert (getU64BE ones 0 == (0 - 1)) "getU64BE of 0xFF..FF == -1"
    assert (shiftR (getU64BE ones 0) 1 == maxI63) "shiftR (-1) 1 logical == 2^63-1"

    putStrLn "bytestring_u64_sign_bit: all 64-bit sign-bit read assertions passed"
