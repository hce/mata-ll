-- One half of a constructor-name pair: this module and DupConWest both
-- declare a constructor named `Shared` — two constructors, which a bare
-- use in a module importing both cannot tell apart. See
-- two_imported_constructors_of_one_name_are_ambiguous_only_where_used
-- (compile_errors.rs).
module DupConEast where

data EastBox = Shared Int

eastGet :: EastBox -> Int
eastGet (Shared n) = n
