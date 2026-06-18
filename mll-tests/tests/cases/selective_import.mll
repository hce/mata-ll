-- Regression test: selective imports must include internal dependencies
-- import Foo (bar) should make bar's internal helpers available for
-- type checking, while hiding them from user code.
-- (Bug: only explicitly named symbols were imported, missing helpers)

import ExportHelper (publicFn)

main :: IO ()
main = do
    -- publicFn internally calls privateFn — that dependency must be resolved
    assert (publicFn 5 == 15) "selective import: publicFn works"
    assert (publicFn 0 == 0) "selective import: publicFn zero"
    putStrLn "All selective import tests passed!"
