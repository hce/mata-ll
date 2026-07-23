-- Regression test: a top-level VALUE binding whose body eagerly reads another
-- top-level binding defined later in the file. The cheap-value codegen path
-- emitted such a binding as an eager `__mll_fn[y] = __mll_fn[x]` (or
-- `x + 1`, `Just g`) at module-load time, capturing nil because the
-- referent's slot was not assigned yet. The fix thunks any cheap value
-- binding that eagerly dereferences a global, deferring the read to first use.
-- (The function-typed analogue is covered by pointfree_caf.)

-- A bare alias to a value defined below.
aliasV :: Int
aliasV = targetV

targetV :: Int
targetV = 42

-- A value defined in terms of a later binding via arithmetic.
derivedV :: Int
derivedV = baseV + 1

baseV :: Int
baseV = 100

-- A constructor field referencing a later binding.
wrappedV :: Maybe Int
wrappedV = Just innerV

innerV :: Int
innerV = 7

unwrap :: Maybe Int -> Int
unwrap (Just n) = n
unwrap Nothing = 0

main :: IO ()
main = do
    assert (aliasV == 42) "forward value alias"
    assert (derivedV == 101) "forward value used in arithmetic"
    assert (unwrap wrappedV == 7) "forward value as a constructor field"
    putStrLn "value_forward_alias ok"
