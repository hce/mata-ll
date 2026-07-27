main :: IO ()
main = do
    -- Right sections
    assert (head (map (+1) [1, 2, 3]) == 2) "right section (+1)"
    assert (head (map (*2) [3, 4, 5]) == 6) "right section (*2)"
    assert (head (filter (>2) [1, 2, 3, 4]) == 3) "right section (>2)"

    -- Left sections
    assert (head (map (10-) [1, 2, 3]) == 9) "left section (10-)"
    assert (head (map (100.0/) [2.0, 4.0, 5.0]) == 50.0) "left section (100.0/)"

    -- Section as value
    let double = (*2)
    assert (double 21 == 42) "section as value"

    -- Compound operands (Haskell 2010 §3.5): a section operand that is an
    -- infix expression must bind tighter than the section operator
    -- (`(+ 2 * 3)`), or chain with it at equal precedence in the section's
    -- own direction — infixl in a left section, infixr in a right section.
    -- The looser/wrong-direction forms are compile errors (covered by
    -- section_operand_precedence_matches_ghc in run_mll.rs).
    assert (head (map (+ 2 * 3) [1]) == 7) "right section, tighter operand"
    assert (head (map (2 * 3 +) [1]) == 7) "left section, tighter operand"
    assert ((2 + 3 +) 1 == 6) "left section, infixl chain"
    assert ((++ [1] ++ [2]) [0] == [0, 1, 2]) "right section, infixr chain"
    assert ((: [1] ++ [2]) 0 == [0, 1, 2]) "right section, cons then append"
    assert ((2 * 3 `div`) 2 == 3) "backtick left section, equal-precedence infixl chain"
    assert ((`div` [4, 2] !! 1) 12 == 6) "backtick right section, tighter operand"
    assert ((-1 +) 3 == 2) "left section, negated operand"
