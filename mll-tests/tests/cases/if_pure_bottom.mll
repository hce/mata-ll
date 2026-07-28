-- The `if` spelling of case_pure_bottom, GHC-parity pin for the
-- concrete-vars seed gate: `if` terminals were already flattened (each
-- branch through `pure_action_ast`), yet this program raised, because the
-- module-level concrete seed listed `undefined` — a runtime THUNK — as a
-- "plain local function", so `pure_value_bare_is_safe` / `is_cheap_to_force`
-- accepted the payload and left it BARE for the consumer's `__mll_run` to
-- force. GHC binds the bottom unforced (`r` is never used) and prints "ok".
-- Fixed 2026-07-28 by removing `undefined` from the seed (module.rs);
-- confirmed against runghc 9.14.1 before pinning.

g :: Int -> IO Int
g n = if n == 0 then pure undefined else pure (n + 1)

main :: IO ()
main = do
  r <- g 0
  s <- g 5
  putStrLn "ok"
  putStrLn (show s)
