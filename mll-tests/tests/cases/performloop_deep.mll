-- Direct-perform IO self-loop conversion (opt pass 7): a 2e6-deep
-- direct-perform accumulator. The single-clause if/else body performs at
-- call time and recurses through `return __mll_run_tail(self(…))` — the
-- self call sits in the runner's ARGUMENT position, so before the
-- conversion every step pinned one interpreter frame and the run
-- overflowed the stack at ~1e6 (verified on PUC 5.5 and LuaJIT). The
-- converted while-loop must run in constant stack, and the exact sum pins
-- the simultaneous parameter update. The `when` guard reads the
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
