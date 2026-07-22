-- Deep tail recursion through case-in-return-position and a let binding.
-- After IIFE flattening (codegen/opt.rs pass 3) the case body is spliced
-- into the function and the recursive call is a direct Lua tail call, so
-- a million steps run in constant stack.

countdown :: Integer -> Integer -> Integer
countdown n acc =
    case n of
        0 -> acc
        _ -> let acc' = acc + 1
             in countdown (n - 1) acc'

main :: IO ()
main = assert (countdown 1000000 0 == 1000000) "case/let tail recursion at depth 10^6"
