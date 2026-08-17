-- Interprocedural direct-perform tails (stage 2 of the bare-tail work):
-- `ping` and `pong` are single-clause direct-perform IO functions that
-- tail-call EACH OTHER. Stage 1 emitted only a SELF tail bare; a tail call
-- to a DIFFERENT function still rode `__mll_run_tail`'s argument position
-- — one pinned Lua frame per crossing, so a 2e6-deep chain died at the
-- ~1e6 frame-per-step limit. With every direct-perform function
-- classified module-wide BEFORE any body is emitted (direct_perform_fns),
-- each crossing is a bare `return pong(...)` / `return ping(...)` — Lua's
-- tail-call form — and the chain runs in constant stack with no loop pass
-- involved (tailloop cannot loop it: it is not self-recursion;
-- perform_bare_tco_mutual_unoptimized in the harness pins the same run
-- with tailloop/ioloop disabled). `done` is a bare-name action
-- terminal as in perform_bare_tco_deep. `pong` is defined AFTER `ping`, so
-- ping's body can only know pong's classification from the pre-pass, not
-- from emission order.

done :: IO Int
done = pure 42

ping :: Int -> IO Int
ping n = if n == 0 then done else pong (n - 1)

pong :: Int -> IO Int
pong n = if n == 0 then done else ping (n - 1)

main :: IO ()
main = do
  r <- ping 2000000
  putStrLn (show r)
