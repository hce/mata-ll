-- Strictness guard for the LBit primitives: an inlined native bit-op must
-- still FORCE a thunked argument, and a truly-demanded bottom through a bit-op
-- must still raise.
--
-- Why this exists: a future optimization that inlines LBit calls to native Lua
-- operators (`band a b` -> `a & b`) must not skip forcing its arguments. If it
-- emitted a bare `a & b` where `a` is an unevaluated thunk (a Lua table), the
-- program would crash with "attempt to perform bitwise operation on a table
-- value" (the arithmetic-path manifestation) rather than compute — exactly the
-- class of bug the lazy_index_thunk_leak / action_result_whnf regressions pin
-- for list heads and <-bound results. The current runtime forces both operands
-- (mllc/src/codegen.rs: __mll_band = `F(a) & F(b)`); this test locks that in.
--
-- CONTRACT: the LBit ops are strict in their operands (they must evaluate them
-- to compute a result). This is the Lua-FFI contract, and it is also what GHC's
-- Data.Bits requires (bit ops are strict) — so both agree here.
--
-- The operands below are GENUINELY thunked (results of recursive functions and
-- self-referential lets), never inline-foldable constants, so the force is real
-- work the optimizer cannot constant-fold away.

import LBit (band, bor, xor, shiftL, shiftR)

-- Recursive: not an inline candidate, so `tri n` is a real thunked application.
-- tri n = n + (n-1) + ... + 1 = n*(n+1)/2. tri 10 = 55.
tri :: Integer -> Integer
tri 0 = 0
tri n = n + tri (n - 1)

-- Another non-foldable thunk source: sum a list built lazily.
sumTo :: Integer -> Integer
sumTo n = go n 0
  where
    go 0 acc = acc
    go k acc = go (k - 1) (acc + k)

-- A bottom that MUST stay unraised until forced, and MUST raise when it is.
boomI :: Integer
boomI = error "boom: a bit-op forced a demanded bottom (correct) — or leaked a thunk"

main :: IO ()
main = do
    -- ============================================================
    -- A thunked operand is forced, and the bit-op computes correctly.
    -- If a native-inline change failed to force `a`, this crashes at runtime
    -- ("bitwise operation on a table value") instead of returning.
    -- ============================================================
    let a = tri 10                       -- thunk -> 55
    assert (band a 255 == 55) "band forces a thunked first operand (tri 10 = 55)"
    let b = tri 10
    assert (bor b 256 == 311) "bor forces a thunked first operand (55 | 256 = 311)"
    let c = sumTo 12                     -- thunk -> 78
    assert (xor c 6 == 72) "xor forces a thunked operand (78 ^ 6 = 72)"
    let d = tri 63                       -- thunk -> 2016, well clear of edges
    assert (shiftR (shiftL d 1) 1 == 2016) "shiftL/shiftR force a thunked operand"

    -- Both operands thunked at once.
    let e = tri 10                       -- 55
    let f = sumTo 5                      -- 15
    assert (band e f == 7) "band forces BOTH thunked operands (55 & 15 = 7)"

    -- Thunk nested one level under another bit-op (force must propagate).
    let g = bor (tri 10) 0               -- thunk over a bit-op -> 55
    assert (band g 255 == 55) "a bit-op forces a thunked bit-op result"

    -- ============================================================
    -- Paired laziness: a truly-DEMANDED bottom through a bit-op STILL raises.
    -- `seq` forces `band boomI 255` to WHNF inside the tried action, so the
    -- error is raised and caught. This proves strictness is real force, not
    -- error-swallowing, and that forcing was not silently elided.
    -- ============================================================
    r1 <- try (seq (band boomI 255) (pure ()))
    case r1 of
        Right _ -> error "band of a demanded bottom must raise, not swallow"
        Left _  -> putStrLn "band of a demanded bottom raises when forced"

    r2 <- try (seq (shiftL boomI 1) (pure ()))
    case r2 of
        Right _ -> error "shiftL of a demanded bottom must raise"
        Left _  -> putStrLn "shiftL of a demanded bottom raises when forced"

    -- ============================================================
    -- ...but a bit-op expression whose RESULT is discarded (never demanded)
    -- must not force its bottom operand. `const` drops it; reaching the
    -- assertion proves no spurious force happened.
    -- ============================================================
    assert (const 1 (band boomI 255) == 1) "discarded bit-op over a bottom stays lazy"

    putStrLn "lbit_strict_primitive_arg: all strictness assertions passed"
