-- The other half of the imported-constructor collision pair — see
-- DupConEast.mll and duplicate_imported_constructor_rejected
-- (compile_errors.rs). The payload type differs on purpose: the collision
-- is on the constructor NAME, not its shape.
module DupConWest where

data WestBox = Shared String

westGet :: WestBox -> String
westGet (Shared s) = s
