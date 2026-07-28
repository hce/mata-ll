-- IO self-loop conversion (opt pass 6): a 2e6-deep IO accumulator loop in
-- the two-level shape (a do-block step, so each step is a branch action
-- closure recursing through the forwarding runner). That shape was
-- already constant-stack through the runner's tail calls; the converted
-- while-loop must stay constant-stack, and the simultaneous parameter
-- update must carry both parameters (the exact sum pins it). The `when`
-- guard reads the accumulator every step, keeping it demanded — the
-- strict-accumulator idiom — so the depth probes the loop structure, not
-- a lazy thunk chain.

sumLoop :: Int -> Int -> IO Int
sumLoop 0 acc = pure acc
sumLoop n acc = do
    when (acc < 0) (putStrLn "never happens")
    sumLoop (n - 1) (acc + n)

main :: IO ()
main = do
    r <- sumLoop 2000000 0
    -- 1+2+...+2000000 = 2000000*2000001/2
    assert (r == 2000001000000) "2e6-deep IO tail accumulator"
