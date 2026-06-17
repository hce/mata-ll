-- FFI strictness tests
-- Verifies that demand analysis correctly propagates strictness through
-- FFI functions (LuaPure/LuaIO), which always force their arguments.

-- Bitwise FFI
xorB :: Integer -> Integer -> LuaPure "__mll_bxor" Integer
bandB :: Integer -> Integer -> LuaPure "__mll_band" Integer
borB :: Integer -> Integer -> LuaPure "__mll_bor" Integer
shlB :: Integer -> Integer -> LuaPure "__mll_shl" Integer
shrB :: Integer -> Integer -> LuaPure "__mll_shr" Integer

-- String FFI
strLen :: String -> LuaPure "string.len" Integer

-- Math FFI
floorF :: Number -> LuaPure "math.floor" Integer

-- ============================================================
-- Core bug: arithmetic on args routed through FFI
-- ============================================================

-- bandB forces (a + b), so a and b must be strict.
-- Before the fix, a + b crashed with "attempt to perform
-- arithmetic on a function value" when a or b were thunks.
add32 :: Integer -> Integer -> Integer
add32 a b = bandB (a + b) 4294967295

-- Same pattern with subtraction
sub32 :: Integer -> Integer -> Integer
sub32 a b = bandB (a - b) 4294967295

-- Same pattern with multiplication
mul32 :: Integer -> Integer -> Integer
mul32 a b = bandB (a * b) 4294967295

-- ============================================================
-- Chained FFI: strictness through multiple FFI layers
-- ============================================================

-- xorB forces both args, so borB's result (which depends on x)
-- must propagate strictness to x.
xorShift :: Integer -> Integer -> Integer
xorShift x n = xorB x (shlB x n)

-- Nested: bandB(borB(shlB x n, shrB x (32-n)), mask)
-- All of x and n must be strict.
rotateL32 :: Integer -> Integer -> Integer
rotateL32 x n = bandB (borB (shlB x n) (shrB x (32 - n))) 4294967295

-- ============================================================
-- FFI in where clauses
-- ============================================================

-- The where-bound `masked` depends on `x` through bandB.
-- `result` depends on `masked` through xorB.
-- So `x` must be strict.
maskAndFlip :: Integer -> Integer
maskAndFlip x = result
  where
    masked = bandB x 255
    result = xorB masked 255

-- ============================================================
-- FFI mixed with pattern matching
-- ============================================================

-- First clause forces via pattern, second forces via FFI chain.
-- Parameter should be strict in both.
clampByte :: Integer -> Integer
clampByte 0 = 0
clampByte n = bandB n 255

-- ============================================================
-- Cross-function: user fn calls FFI-using fn
-- ============================================================

-- doubleAnd calls add32, which is strict (through FFI).
-- So doubleAnd's args should also be strict.
doubleAnd :: Integer -> Integer -> Integer
doubleAnd a b = add32 (add32 a b) b

-- ============================================================
-- FFI result used in comparison (another forcing context)
-- ============================================================

hasBitSet :: Integer -> Integer -> Bool
hasBitSet val bit = bandB val (shlB 1 bit) /= 0

-- ============================================================
-- FFI with let bindings creating thunk chains
-- ============================================================

-- Each let creates a thunk. The final bandB must force through
-- the whole chain: z depends on y, y depends on x, etc.
thunkChain :: Integer -> Integer -> Integer
thunkChain a b =
    let x = xorB a b
        y = shlB x 3
        z = bandB y 255
    in z

-- ============================================================
-- Negate through FFI
-- ============================================================

-- negate is a builtin op; result passed to bandB
negMask :: Integer -> Integer
negMask x = bandB (0 - x) 4294967295

main :: IO ()
main = do
    -- Core bug: arithmetic on FFI-routed args
    assert (add32 100 200 == 300) "add32 basic"
    assert (add32 4294967295 1 == 0) "add32 overflow wraps"
    assert (add32 4294967295 4294967295 == 4294967294) "add32 double max"
    assert (sub32 100 30 == 70) "sub32 basic"
    assert (mul32 65536 65536 == 0) "mul32 overflow wraps"

    -- Chained FFI
    assert (xorShift 1 4 == 17) "xorShift 1<<4 xor 1"
    assert (rotateL32 1 8 == 256) "rotateL32 1 by 8"
    assert (rotateL32 2147483648 1 == 1) "rotateL32 high bit wraps"

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
    assert (negMask 1 == 4294967295) "negMask 1"
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
