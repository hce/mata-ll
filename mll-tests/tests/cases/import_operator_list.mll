-- Operators in import lists: `import M ((&), f)` and
-- `import M hiding ((|>))`.  Regression: the export list accepted the
-- parenthesized-operator spelling but the import and hiding lists
-- rejected it with "Expected identifier".  (That a hidden operator
-- stays rejected on use is covered by the '&'-hidden probe in
-- compile_errors.rs.)

import OpsExports ((&), pipeApply)

main :: IO ()
main = do
    assert ((5 & (\v -> v * 2)) == 10) "operator via specific import list"
    assert (pipeApply 7 == 8) "named import in the same list"
    putStrLn "operator import list ok"
