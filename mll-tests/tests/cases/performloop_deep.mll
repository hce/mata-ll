-- Direct-perform IO self-recursion, 2e6 deep: a direct-perform
-- accumulator whose single-clause if/else body performs at call time and
-- recurses at its action tail. Originally this emitted `return
-- __mll_run_tail(self(…))` — the self call in the runner's ARGUMENT
-- position, one pinned interpreter frame per step, stack overflow at ~1e6
-- (verified on PUC 5.5 and LuaJIT) — and a dedicated loop pass
-- (performloop, opt pass 7) converted it. The emitter now spells the
-- saturated tail as a bare `return self(…)` (action.rs / direct_perform_fns),
-- Lua's own tail call, which tailloop turns into a while-loop; the pass was
-- retired 2026-08-17. The run must stay constant-stack, and the exact sum
-- pins the simultaneous parameter update. The `when` guard reads the
-- accumulator every step (the strict-accumulator idiom), so the depth
-- probes the loop structure, not a lazy thunk chain.

sumLoop :: Int -> Int -> IO Int
sumLoop n acc =
  if n == 0
    then pure acc
    else do
      when (acc < 0) (putStrLn "never happens")
      sumLoop (n - 1) (acc + n)

main :: IO ()
main = do
  r <- sumLoop 2000000 0
  -- 1+2+...+2000000 = 2000000*2000001/2
  assert (r == 2000001000000) "2e6-deep direct-perform accumulator"
