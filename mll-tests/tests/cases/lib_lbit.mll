import LBit (xor, band, bor, bnot, shiftL, shiftR)

main :: IO ()
main = do
    -- band
    assert (band 0 0 == 0) "band 0 0"
    assert (band 255 0 == 0) "band 255 0"
    assert (band 255 255 == 255) "band 255 255"
    assert (band 12 10 == 8) "band 12 10 = 8"
    assert (band 7 3 == 3) "band 7 3 = 3"

    -- bor
    assert (bor 0 0 == 0) "bor 0 0"
    assert (bor 12 10 == 14) "bor 12 10 = 14"
    assert (bor 1 2 == 3) "bor 1 2 = 3"
    assert (bor 5 3 == 7) "bor 5 3 = 7"

    -- xor
    assert (xor 0 0 == 0) "xor 0 0"
    assert (xor 255 255 == 0) "xor self = 0"
    assert (xor 255 0 == 255) "xor 255 0"
    assert (xor 12 10 == 6) "xor 12 10 = 6"
    assert (xor 5 3 == 6) "xor 5 3 = 6"

    -- bnot (bitwise complement)
    -- Lua 5.4 integers are 64-bit, so bnot 0 = -1 (all bits set)
    assert (bnot 0 == -1) "bnot 0 = -1"
    assert (bnot (-1) == 0) "bnot -1 = 0"
    assert (band (bnot 255) 255 == 0) "bnot 255 & 255 = 0"

    -- shiftL
    assert (shiftL 1 0 == 1) "shl 1 0"
    assert (shiftL 1 1 == 2) "shl 1 1"
    assert (shiftL 1 4 == 16) "shl 1 4"
    assert (shiftL 3 2 == 12) "shl 3 2"
    assert (shiftL 5 3 == 40) "shl 5 3"

    -- shiftR
    assert (shiftR 16 4 == 1) "shr 16 4"
    assert (shiftR 255 4 == 15) "shr 255 4"
    assert (shiftR 1 0 == 1) "shr 1 0"
    assert (shiftR 12 2 == 3) "shr 12 2"

    -- combined operations
    assert (xor (band 15 12) (bor 1 2) == 15) "combined: (15&12) xor (1|2) = 12 xor 3 = 15"
    assert (shiftL (band 255 3) 4 == 48) "combined: (255&3)<<4 = 3<<4 = 48"
