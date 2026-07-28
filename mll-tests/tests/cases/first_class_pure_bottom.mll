-- First-class `pure` bottom reaching a forwarding runner, GHC-parity pin:
-- `id (pure undefined)` inlines to the first-class pure-action closure
-- (expr.rs's return/pure arm), which the direct-perform tail hands to
-- `__mll_run_tail`. Left bare, the closure returned the raw payload thunk
-- and the consumer's `__mll_run` forced it, raising where GHC binds the
-- bottom unforced — `r` is never used, so GHC prints "ok". Fixed
-- 2026-07-28: the first-class closure's payload takes the same
-- `pure_action_ast` escape decision as an escaping terminal, so an unsafe
-- payload crosses in a `__mll_pure` box that only the one consuming unbox
-- strips. Confirmed against runghc 9.14.1 before pinning.

g :: Int -> IO Int
g n = id (pure undefined)

h :: Int -> IO Int
h n = id (pure (n + 1))

main :: IO ()
main = do
  r <- g 0
  s <- h 4
  putStrLn "ok"
  putStrLn (show s)
