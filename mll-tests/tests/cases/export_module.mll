-- Tests for export keyword and module export control

import ExportHelper

-- ============================================================
-- Export keyword: marks functions for Lua interop
-- ============================================================

-- A helper function used by the exported function
helper :: Integer -> Integer
helper n = n * 10

-- Export declaration (also serves as type signature)
export greet :: String -> String
greet name = "Hello, " ++ name ++ "!"

export compute :: Integer -> Integer
compute n = helper n + 1

-- ============================================================
-- Module export control: ExportHelper exposes publicFn and
-- PublicType, but hides privateFn and PrivateType
-- ============================================================

main :: IO ()
main = do
    -- Exported functions work
    assert (greet "world" == "Hello, world!") "export greet"
    assert (compute 5 == 51) "export compute"

    -- Module imports: public items are accessible
    assert (publicFn 3 == 9) "module public fn"
    assert (PubA == PubA) "module public type A"
    assert (PubB 5 == PubB 5) "module public type B"
    assert (PubA /= PubB 1) "module public type neq"

    -- Note: privateFn and PrivateType should be rejected by the
    -- typechecker if referenced here. We can't test negative cases
    -- in this harness, but the positive tests confirm the module
    -- system works for allowed exports.
