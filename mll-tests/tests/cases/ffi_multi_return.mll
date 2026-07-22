-- A host function that returns multiple values, declared with a
-- single-value result type. The declared type is the contract: the FFI
-- wrapper must truncate to the first value, so the extras can never
-- spread into a consuming position (an unparenthesized Lua call in
-- last-argument or return position forwards ALL its values).

modf1 :: Number -> LuaPure "math.modf" Number
countArgs :: String -> Number -> LuaPure "select" Number

pass2 :: Number -> Number -> Number
pass2 _ b = b

main :: IO ()
main = do
    -- Constructed (non-literal) argument so the marshalling path is real.
    let x = 3.0 + 0.75
    assert (modf1 x == 3.0) "wrapper yields the first value only"
    assert (pass2 1.0 (modf1 x) == 3.0) "last-argument position sees one value"
    assert (countArgs "#" (modf1 x) == 1.0) "no spread into a counting host"
