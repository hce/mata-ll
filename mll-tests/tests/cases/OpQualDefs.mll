-- Helper module for qualified_operator_module.mll: defines an operator
-- AND uses it infix in sibling bodies (plus a backtick-infix sibling
-- call). Under `import qualified … as Q` the operator's definition is
-- prefixed like every value, so these in-module infix uses must be
-- prefixed too — the InfixApp op rewrite this module regression-tests.
module OpQualDefs (combined, addBoth) where

(<+>) :: Int -> Int -> Int
a <+> b = a + b + 1

combine :: Int -> Int -> Int
combine a b = a <+> b

combined :: Int
combined = 3 <+> 4

addBoth :: Int -> Int -> Int
addBoth a b = a `combine` b
