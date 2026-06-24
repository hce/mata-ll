-- `seq` forces its first argument, then yields the second. When the second is
-- a tail call, codegen must keep it a Lua tail call (`return f x`, not the
-- non-tail `return (f x)`), or deep seq-strict recursion overflows the stack.
-- This drives a strict accumulator far past any reasonable C-stack depth.

sumStrict :: Integer -> Integer -> Integer
sumStrict 0 acc = acc
sumStrict n acc = seq acc (sumStrict (n - 1) (acc + n))

-- `seq` also forces an unevaluated thunk to WHNF before returning the result.
forceFirst :: Integer
forceFirst = seq (1 + 2) 99

main :: IO ()
main = do
    -- 1+2+...+2000000 = 2000000*2000001/2 = 2000001000000
    assert (sumStrict 2000000 0 == 2000001000000) "seq-strict deep tail recursion"
    assert (forceFirst == 99) "seq returns its second argument"
