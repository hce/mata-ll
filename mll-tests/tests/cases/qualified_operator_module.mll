-- Qualified-importing a module that defines AND infix-uses an operator.
-- Regression: the qualified-prefix rewriter renamed the operator's
-- DEFINITION (`<+>` → `Q.<+>`) but never rewrote InfixApp op names, so
-- the module's own `a <+> b` (and backtick `a `combine` b`) resolved to
-- nothing: "Unbound variable: (<+>)" (confirmed by repro).

import qualified OpQualDefs as Q

main :: IO ()
main = do
    assert (Q.combined == 8) "in-module infix operator use"
    assert (Q.addBoth 10 20 == 31) "in-module backtick infix use"
    putStrLn "qualified operator module ok"
