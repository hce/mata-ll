-- ZBytes: fixed-width integer readers over ByteStrings, in both byte orders.
--
-- ZFS on-disk structures are written in the byte order of the host that
-- created the pool, so every structure read is parameterized over Endian
-- (detected once from the uberblock magic). A few formats are fixed-order
-- regardless of host: XDR nvlists and fat-ZAP name/value arrays are always
-- big-endian, and the ZFS LZ4 frame header is big-endian — those callers use
-- the *BE readers directly.
--
-- All offsets are 0-based byte offsets, matching the ByteString intrinsics.

module ZBytes
    ( Endian(..)
    , getU16, getU32, getU64
    , getU16BE, getU32BE, getU64BE
    , getU64LE
    , groupBE
    , cstringAt
    ) where

import LBit (bor, shiftL)

data Endian = LE | BE
    deriving (Show, Eq)

-- Little-endian 64-bit read, built from the 32-bit intrinsic. The result is
-- a native Lua 5.4 64-bit integer; values with the top bit set come out
-- negative, which is fine — all consumers mask/shift rather than compare
-- magnitudes.
getU64LE :: ByteString -> Integer -> Integer
getU64LE b off = bor (bsGetU32LE b off) (shiftL (bsGetU32LE b (off + 4)) 32)

getU16BE :: ByteString -> Integer -> Integer
getU16BE b off = bsIndex b off * 256 + bsIndex b (off + 1)

getU32BE :: ByteString -> Integer -> Integer
getU32BE b off = shiftL (getU16BE b off) 16 + getU16BE b (off + 2)

getU64BE :: ByteString -> Integer -> Integer
getU64BE b off = bor (shiftL (getU32BE b off) 32) (getU32BE b (off + 4))

-- Byte-order-dispatched readers for native-order structures.
getU16 :: Endian -> ByteString -> Integer -> Integer
getU16 LE b off = bsGetU16LE b off
getU16 BE b off = getU16BE b off

getU32 :: Endian -> ByteString -> Integer -> Integer
getU32 LE b off = bsGetU32LE b off
getU32 BE b off = getU32BE b off

getU64 :: Endian -> ByteString -> Integer -> Integer
getU64 LE b off = getU64LE b off
getU64 BE b off = getU64BE b off

-- Split a buffer into consecutive big-endian integers of `width` bytes
-- (fat-ZAP value arrays store their integers big-endian on every host).
groupBE :: Integer -> ByteString -> [Integer]
groupBE width b = go 0
  where
    n = bsLength b
    go off =
        if off + width > n
        then []
        else beInt off width 0 : go (off + width)
    beInt off k acc =
        if k == 0
        then acc
        else beInt (off + 1) (k - 1) (acc * 256 + bsIndex b off)

-- NUL-terminated string starting at `off`, at most `maxLen` bytes.
cstringAt :: ByteString -> Integer -> Integer -> String
cstringAt b off maxLen = bsToString (bsSub b off (scan 0))
  where
    limit =
        let avail = bsLength b - off
        in if maxLen < avail then maxLen else avail
    scan i =
        if i >= limit
        then i
        else if bsIndex b (off + i) == 0
             then i
             else scan (i + 1)
