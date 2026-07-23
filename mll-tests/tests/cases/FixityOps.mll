-- Operator-exporting module for fixity_import.mll. Fixity travels with the
-- export, exactly as in GHC: the importing module must parse chains of these
-- operators under the fixities declared here, not under the infixl 9
-- default for undeclared operators.
module FixityOps ((-.), (~=~)) where

infixr 6 -.
(-.) :: Int -> Int -> Int
a -. b = a - b

infix 4 ~=~
(~=~) :: Int -> Int -> Bool
a ~=~ b = a == b
