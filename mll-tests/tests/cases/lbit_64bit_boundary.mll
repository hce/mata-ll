-- lua-compat-skip: luajit
--   (this case asserts 64-bit values with the top bit set; LuaJIT's doubles
--   cannot represent integers > 2^53, so the sign-bit magics and shifts are
--   rounded before any op runs — a documented limitation, see
--   doc/articles/CAVEATS.md. The contract holds on 64-bit-integer hosts.)
-- LBit at the 64-bit / sign-bit boundary.
--
-- CONTRACT (established from source, not assumed):
--   LBit is NOT Data.Bits parity. It is the Lua-semantics FFI family (the "L"
--   prefix, like LIO/LOS/LIOLinear), where FFI is the stated GHC-parity
--   exception. Each op in mllc/lib/LBit.mll is a `LuaPure "__mll_..."` binding,
--   and mllc/src/codegen.rs emits them as NATIVE Lua 5.4 operators:
--       band = &   bor = |   xor = ~   bnot = ~   shiftL = <<   shiftR = >>
--   So the contract is Lua 5.4 integer semantics on 64-bit signed integers:
--     * shiftR is LOGICAL (zero-fill), NOT arithmetic — it does NOT sign-extend.
--     * a shift count with absolute value >= 64 yields 0 (all bits shift out).
--       (Haskell's Data.Bits leaves shiftL/shiftR at counts >= finiteBitSize
--       effectively unspecified for a fixed-width type; we do NOT pretend GHC
--       dictates this — we pin Lua's documented behavior, which is the contract
--       LBit binds to. Lua 5.4 manual 3.4.3: displacements >= word size give 0.)
--
-- These are exactly the values the zpool reader depends on (block-pointer word
-- decode, the ZAP block-type magics 0x8000000000000003 / 0x8000000000000001,
-- the 2^63-1 offset mask). The lexer has no hex literals, so the sign-bit
-- constants are built with shiftL/bor, and 2^63 (out of signed range as a
-- literal) is written as (0 - maxI63 - 1), matching the existing corpus style.
--
-- Every value below was computed from the Lua 5.4 reference, then encoded as
-- the CORRECT contract value — not read back from mllc's output.

import LBit (xor, band, bor, bnot, shiftL, shiftR)

-- 2^63 - 1, the largest positive 64-bit signed integer.
maxI63 :: Int
maxI63 = 9223372036854775807

-- -2^63, the most-negative 64-bit signed integer (= 1 << 63 as a bit pattern).
minI63 :: Int
minI63 = 0 - 9223372036854775807 - 1

main :: IO ()
main = do
    -- ============================================================
    -- shiftL producing / around the sign bit
    -- ============================================================
    -- 1 << 63 sets ONLY the sign bit; as a signed 64-bit int that is -2^63.
    assert (shiftL 1 63 == minI63) "shiftL 1 63 == -2^63 (sign bit set)"
    -- 1 << 62 is the largest single-bit positive power of two.
    assert (shiftL 1 62 == 4611686018427387904) "shiftL 1 62 == 2^62"
    assert (shiftL 1 0 == 1) "shiftL 1 0 == 1 (identity)"
    assert (shiftL 3 62 == minI63 + 4611686018427387904)
        "shiftL 3 62 overflows into the sign bit (bit 63 and bit 62 set)"

    -- ============================================================
    -- Shift-count edges: 0, 63, and >= 64 (the Lua contract, see header)
    -- ============================================================
    assert (shiftL 1 64 == 0) "shiftL by 64 == 0 (count >= word size, Lua contract)"
    assert (shiftL 1 65 == 0) "shiftL by 65 == 0 (count > word size)"
    assert (shiftR (shiftL 1 63) 64 == 0) "shiftR by 64 == 0 (count >= word size)"
    assert (shiftR minI63 0 == minI63) "shiftR by 0 == identity (even with sign bit set)"
    assert (shiftL minI63 0 == minI63) "shiftL by 0 == identity"

    -- ============================================================
    -- shiftR is LOGICAL (zero-fill), not arithmetic — the key divergence
    -- from Data.Bits. On a sign-bit-set operand it must NOT sign-extend.
    -- ============================================================
    -- (1 << 63) >> 63 brings the sign bit down to bit 0 with zero fill == 1.
    -- An arithmetic (sign-extending) shift would give -1 here; it must not.
    assert (shiftR (shiftL 1 63) 63 == 1) "shiftR of sign bit is logical (== 1, not -1)"
    -- -1 is all ones; >> 1 zero-fills the top bit -> 2^63 - 1 (arithmetic: -1).
    assert (shiftR (0 - 1) 1 == maxI63) "shiftR (-1) 1 is logical (== 2^63-1, not -1)"
    assert (shiftR (0 - 1) 63 == 1) "shiftR (-1) 63 is logical (== 1, not -1)"

    -- ============================================================
    -- band / bor / xor on operands with the top bit set (Lua-negative)
    -- ============================================================
    assert (band minI63 minI63 == minI63) "band of sign-bit operand with itself"
    assert (band minI63 maxI63 == 0) "band sign-bit & low-63-bits == 0 (disjoint)"
    assert (bor minI63 0 == minI63) "bor sign-bit | 0 preserves the sign bit"
    assert (bor minI63 maxI63 == (0 - 1)) "bor sign-bit | low-63 == all ones (-1)"
    assert (xor minI63 minI63 == 0) "xor of a sign-bit operand with itself == 0"
    assert (xor minI63 maxI63 == (0 - 1)) "xor sign-bit ^ low-63 == all ones (-1)"

    -- ============================================================
    -- bnot (Lua ~, 64-bit complement)
    -- ============================================================
    assert (bnot 0 == (0 - 1)) "bnot 0 == -1 (all bits set)"
    assert (bnot (0 - 1) == 0) "bnot -1 == 0"
    assert (bnot minI63 == maxI63) "bnot (-2^63) == 2^63-1 (flip the sign bit)"
    assert (bnot maxI63 == minI63) "bnot (2^63-1) == -2^63"

    -- ============================================================
    -- The exact masks/magics the zpool reader relies on
    -- ============================================================
    -- mzapBlockType = 0x8000000000000003 = (1<<63) | 3. As signed: -2^63 + 3.
    assert (bor (shiftL 1 63) 3 == minI63 + 3) "mzap block type 0x80..03 == -2^63+3"
    -- fzapBlockType = 0x8000000000000001 = (1<<63) | 1.
    assert (bor (shiftL 1 63) 1 == minI63 + 1) "fzap block type 0x80..01 == -2^63+1"
    -- zbtLeaf = 0x8000000000000000 = 1<<63.
    assert (shiftL 1 63 == minI63) "zap-leaf block type 0x80..00 == -2^63"
    -- The reader's low-63-bits offset mask, built the same way it is in ZPool:
    -- (1<<63) wraps to min int, minus 1 wraps back to max int == 2^63-1.
    assert (shiftL 1 63 - 1 == maxI63) "offset mask (1<<63)-1 == 2^63-1"
    -- Recovering the low bits out of the sign-bit-set magic (as the reader does
    -- when it strips the block-type high bits).
    assert (band (bor (shiftL 1 63) 3) (shiftL 1 63 - 1) == 3)
        "band magic (2^63-1) recovers the low bits (3)"
    -- ZPL directory-entry decode shape: object id in low 48 bits, type in the
    -- top nibble. For 0x8000000000000003: low-48 == 3, top nibble == 8 (dtReg).
    assert (band (bor (shiftL 1 63) 3) 281474976710655 == 3) "entry object id (low 48 bits)"
    assert (band (shiftR (bor (shiftL 1 63) 3) 60) 15 == 8) "entry type (top nibble) == 8"

    putStrLn "lbit_64bit_boundary: all sign-bit / shift-edge assertions passed"
