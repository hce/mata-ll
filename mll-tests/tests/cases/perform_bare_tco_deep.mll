-- Bare-name-terminal depth pin: `done` is a bare-name action terminal — a
-- shape the (since retired) performloop pass declined, so before the bare
-- self-tail emission it kept the frame-per-step ~1e6 depth limit
-- (`return __mll_run_tail(self(...))` pinned the frame in the runner's
-- argument position). The direct-perform emission spells the saturated
-- self site as a bare `return deep(...)` — the exact form Lua's tail-call
-- elimination reclaims the frame for — so raw TCO alone carries a 2e6-deep
-- run in constant stack even with tailloop/ioloop disabled (pinned by
-- perform_bare_tco_deep_unoptimized in the harness, which compiles with
-- CompileOptions::disable_opt_passes).

done :: IO Int
done = pure 42

deep :: Int -> IO Int
deep n = if n == 0 then done else deep (n - 1)

main :: IO ()
main = do
  r <- deep 2000000
  putStrLn (show r)
