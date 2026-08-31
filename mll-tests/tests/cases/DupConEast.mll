-- One half of the imported-constructor collision pair: this module and
-- DupConWest both declare a constructor named `Shared`. A root importing
-- both trips the non-local arm of the typechecker's
-- claim_constructor_name — two imports have no shadowing order, so the
-- merged flat namespace would silently hand the name to one of them.
-- See duplicate_imported_constructor_rejected (compile_errors.rs).
module DupConEast where

data EastBox = Shared Int

eastGet :: EastBox -> Int
eastGet (Shared n) = n
