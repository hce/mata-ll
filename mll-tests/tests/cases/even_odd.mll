-- Prelude even/odd, GHC's exact definitions over the Integral class:
--   even n = n `rem` 2 == 0
--   odd    = not . even
-- `rem` truncates toward zero, so (-3) `rem` 2 is -1 (not 1); the parity
-- answer is still right because only the ==-0 comparison matters. The
-- cases below pin the negative-argument path, zero, first-class use
-- (map even), laziness through an infinite list, and use through a
-- caller's own Integral constraint.

-- even/odd reached through a polymorphic Integral-constrained function,
-- not just at a concrete Integer use site.
parityName :: Integral a => a -> String
parityName n = if even n then "even" else "odd"

-- Both predicates through the same constrained context.
agree :: Integral a => a -> Bool
agree n = even n /= odd n

main :: IO ()
main = do
    -- Zero and small positives.
    assert (even 0)         "even 0"
    assert (not (odd 0))    "odd 0 is False"
    assert (even 2)         "even 2"
    assert (odd 1)          "odd 1"
    assert (not (even 7))   "even 7 is False"
    assert (odd 7)          "odd 7"

    -- Negatives: rem truncates toward zero, so the remainder of a
    -- negative argument is 0 or negative — parity must still be right.
    assert (even (-4))       "even -4"
    assert (not (even (-3))) "even -3 is False"
    assert (odd (-3))        "odd -3"
    assert (not (odd (-4)))  "odd -4 is False"
    assert (even (-0))       "even -0"

    -- The HASKDIFF closing example: filter over an infinite list.
    assert (take 10 (filter even [1 ..]) == [2, 4, 6, 8, 10, 12, 14, 16, 18, 20])
        "take 10 (filter even [1 ..])"
    assert (take 5 (filter odd [1 ..]) == [1, 3, 5, 7, 9])
        "take 5 (filter odd [1 ..])"

    -- First-class use: the predicates passed as values, not applied.
    assert (map even [-2, -1, 0, 1, 2] == [True, False, True, False, True])
        "map even"
    assert (map odd [-2, -1, 0, 1, 2] == [False, True, False, True, False])
        "map odd"
    assert (all even [0, 2, -8, 100]) "all even"
    assert (any odd [2, 4, 5])        "any odd"

    -- Through a caller's own Integral constraint.
    assert (parityName (-6) == "even") "parityName -6"
    assert (parityName 9 == "odd")     "parityName 9"
    assert (agree (-5) && agree 0 && agree 12) "even and odd always disagree"

    -- Large exact integers: parity survives beyond the double mantissa
    -- (2^53 is even, 2^53 + 1 is odd).
    assert (even 9007199254740992)      "even 2^53"
    assert (odd 9007199254740993)       "odd 2^53+1"
    assert (odd (-9007199254740993))    "odd -(2^53+1)"

    putStrLn "even_odd ok"
