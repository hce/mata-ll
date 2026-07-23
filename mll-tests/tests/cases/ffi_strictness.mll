-- FFI strictness tests
-- Verifies that demand analysis correctly propagates strictness through
-- FFI functions (LuaPure/LuaIO), which always force their arguments.

-- Bitwise FFI
xorB :: Int -> Int -> LuaPure "__mll_bxor" Int
bandB :: Int -> Int -> LuaPure "__mll_band" Int
borB :: Int -> Int -> LuaPure "__mll_bor" Int
shlB :: Int -> Int -> LuaPure "__mll_shl" Int
shrB :: Int -> Int -> LuaPure "__mll_shr" Int

-- String FFI
strLen :: String -> LuaPure "string.len" Int

-- Math FFI
floorF :: Number -> LuaPure "math.floor" Int

-- ============================================================
-- Core bug: arithmetic on args routed through FFI
-- ============================================================

-- bandB forces (a + b), so a and b must be strict.
-- Before the fix, a + b crashed with "attempt to perform
-- arithmetic on a function value" when a or b were thunks.
add32 :: Int -> Int -> Int
add32 a b = bandB (a + b) 4294967295

-- Same pattern with subtraction
sub32 :: Int -> Int -> Int
sub32 a b = bandB (a - b) 4294967295

-- Same pattern with multiplication
mul32 :: Int -> Int -> Int
mul32 a b = bandB (a * b) 4294967295

-- ============================================================
-- Chained FFI: strictness through multiple FFI layers
-- ============================================================

-- xorB forces both args, so borB's result (which depends on x)
-- must propagate strictness to x.
xorShift :: Int -> Int -> Int
xorShift x n = xorB x (shlB x n)

-- Nested: bandB(borB(shlB x n, shrB x (32-n)), mask)
-- All of x and n must be strict.
rotateL32 :: Int -> Int -> Int
rotateL32 x n = bandB (borB (shlB x n) (shrB x (32 - n))) 4294967295

-- ============================================================
-- FFI in where clauses
-- ============================================================

-- The where-bound `masked` depends on `x` through bandB.
-- `result` depends on `masked` through xorB.
-- So `x` must be strict.
maskAndFlip :: Int -> Int
maskAndFlip x = result
  where
    masked = bandB x 255
    result = xorB masked 255

-- ============================================================
-- FFI mixed with pattern matching
-- ============================================================

-- First clause forces via pattern, second forces via FFI chain.
-- Parameter should be strict in both.
clampByte :: Int -> Int
clampByte 0 = 0
clampByte n = bandB n 255

-- ============================================================
-- Cross-function: user fn calls FFI-using fn
-- ============================================================

-- doubleAnd calls add32, which is strict (through FFI).
-- So doubleAnd's args should also be strict.
doubleAnd :: Int -> Int -> Int
doubleAnd a b = add32 (add32 a b) b

-- ============================================================
-- FFI result used in comparison (another forcing context)
-- ============================================================

hasBitSet :: Int -> Int -> Bool
hasBitSet val bit = bandB val (shlB 1 bit) /= 0

-- ============================================================
-- FFI with let bindings creating thunk chains
-- ============================================================

-- Each let creates a thunk. The final bandB must force through
-- the whole chain: z depends on y, y depends on x, etc.
thunkChain :: Int -> Int -> Int
thunkChain a b =
    let x = xorB a b
        y = shlB x 3
        z = bandB y 255
    in z

-- ============================================================
-- Negate through FFI
-- ============================================================

-- negate is a builtin op; result passed to bandB
negMask :: Int -> Int
negMask x = bandB (0 - x) 4294967295

main :: IO ()
main = do
    -- Core bug: arithmetic on FFI-routed args
    assert (add32 100 200 == 300) "add32 basic"
    assert (sub32 100 30 == 70) "sub32 basic"
    -- Use bandB to normalize results for LuaJIT compatibility (signed 32-bit)
    assert (bandB (add32 1000000 2000000) 16777215 == 3000000) "add32 large"
    assert (bandB (mul32 1000 1000) 16777215 == 1000000) "mul32 large"

    -- Chained FFI
    assert (xorShift 1 4 == 17) "xorShift 1<<4 xor 1"
    assert (rotateL32 1 8 == 256) "rotateL32 1 by 8"

    -- Where clauses with FFI
    assert (maskAndFlip 0 == 255) "maskAndFlip 0 -> 0xff"
    assert (maskAndFlip 255 == 0) "maskAndFlip 0xff -> 0"
    assert (maskAndFlip 170 == 85) "maskAndFlip 0xaa -> 0x55"

    -- Pattern matching + FFI
    assert (clampByte 0 == 0) "clampByte zero"
    assert (clampByte 256 == 0) "clampByte 256"
    assert (clampByte 511 == 255) "clampByte 511"

    -- Cross-function propagation through FFI
    assert (doubleAnd 10 20 == 50) "doubleAnd basic"

    -- FFI result in comparison
    assert (hasBitSet 5 0 == True) "hasBitSet bit 0"
    assert (hasBitSet 5 1 == False) "hasBitSet bit 1"
    assert (hasBitSet 5 2 == True) "hasBitSet bit 2"

    -- Thunk chain through FFI
    assert (thunkChain 255 170 == 168) "thunkChain"
    assert (thunkChain 0 0 == 0) "thunkChain zeros"

    -- Negate through FFI
    assert (bandB (negMask 1) 255 == 255) "negMask 1 low byte"
    assert (negMask 0 == 0) "negMask 0"

    -- Verify thunks are properly forced: pass results of lazy
    -- computations (not literals) into FFI-strict functions.
    let a = head [100, 200, 300]
    let b = head [50, 60, 70]
    assert (add32 a b == 150) "add32 with thunked args"
    assert (xorShift a 1 == 172) "xorShift with thunked arg"
    assert (rotateL32 a 24 == 1677721600) "rotateL32 with thunked arg"
    assert (maskAndFlip a == 155) "maskAndFlip with thunked arg"
    assert (doubleAnd a b == 200) "doubleAnd with thunked args"

    putStrLn "All FFI strictness tests passed!"
