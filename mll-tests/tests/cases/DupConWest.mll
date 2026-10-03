-- The other half of the constructor-name pair — see DupConEast.mll and
-- two_imported_constructors_of_one_name_are_ambiguous_only_where_used
-- (compile_errors.rs). The payload type differs on purpose: the shared
-- part is the constructor NAME, not its shape.
module DupConWest where

data WestBox = Shared String

westGet :: WestBox -> String
westGet (Shared s) = s
