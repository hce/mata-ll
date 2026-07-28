-- Self-tail-call → loop conversion (opt pass 5): a 1e6-deep strict
-- accumulator loop. Without either Lua's proper tail calls or the loop
-- conversion this would overflow the interpreter stack; with the
-- conversion it must also produce the exact sum, pinning that the
-- simultaneous parameter update carries both parameters correctly.

sumAcc :: Int -> Int -> Int
sumAcc 0 acc = acc
sumAcc n acc = sumAcc (n - 1) (acc + n)

-- The where-group spelling of the same loop (`go = function(...)` header),
-- covering the second converted header form.
sumTo :: Int -> Int
sumTo n = go n 0
  where
    go 0 acc = acc
    go k acc = go (k - 1) (acc + k)

main :: IO ()
main = do
    -- 1+2+...+1000000 = 1000000*1000001/2
    assert (sumAcc 1000000 0 == 500000500000) "1e6-deep tail accumulator"
    assert (sumTo 1000000 == 500000500000) "1e6-deep where-group tail accumulator"
