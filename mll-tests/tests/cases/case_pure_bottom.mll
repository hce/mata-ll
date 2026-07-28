-- Non-recursive case-dispatch `pure` bottom, GHC-parity pin: `g 0` reaches
-- `pure undefined` through a case terminal, and GHC binds the bottom
-- UNFORCED — `r` is never used, so nothing raises and "ok" prints. No loop
-- pass can ever repair this shape (there is no self site), so the emission
-- itself must be right. Two generation decisions used to break it: the case
-- terminal rode a dispatch IIFE into `__mll_run_tail`'s argument as a
-- first-class pure-suspension closure (not the `pure_action_ast` box), and
-- `undefined` was wrongly seeded concrete (it is a runtime THUNK), so the
-- payload escaped bare and the consumer's `__mll_run` forced it. Both fixed
-- 2026-07-28: case terminals flatten at statement level (each branch boxes
-- its own `pure`), and the concrete-vars seed no longer claims `undefined`.
-- Confirmed against runghc 9.14.1 before pinning.

g :: Int -> IO Int
g n = case n of
  0 -> pure undefined
  _ -> pure (n + 1)

main :: IO ()
main = do
  r <- g 0
  s <- g 1
  putStrLn "ok"
  putStrLn (show s)
