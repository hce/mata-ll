-- Imported operators carry their declared fixity into this module, as in
-- GHC. FixityOps declares `infixr 6 -.`; before the fix the importing module
-- fell back to the infixl 9 default, so `10 -. 3 -. 2` grouped left (= 5)
-- instead of right (= 9). The non-associative `~=~` (infix 4) from the same
-- module is rejected in chains here too — see the compile-error tests in
-- run_mll.rs.

import FixityOps

-- A local operator whose fixity declaration appears at the bottom of the
-- file: a fixity declaration governs its whole scope, including uses that
-- precede it textually.
(//) :: Int -> Int -> Int
a // b = a - b

main :: IO ()
main = do
    assert (10 -. 3 -. 2 == 9) "imported infixr 6 groups right"
    assert (1 ~=~ 1) "imported infix 4 operator applies"
    assert ((1 ~=~ 2) == False) "imported infix 4 operator, parenthesized chain"
    assert (10 // 3 // 2 == 9) "fixity declaration applies file-wide, not from its line down"
    putStrLn "fixity_import ok"

infixr 6 //
