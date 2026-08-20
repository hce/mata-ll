-- `quot` and `rem` are infixl 7 in GHC's Prelude — the same level as
-- * / `div` `mod`, grouping left with them.
-- Regression: they had no declared or default fixity and fell to the
-- infixl 9 default, so `4 * 5 \`rem\` 3` parsed as `4 * (5 \`rem\` 3)` = 8
-- instead of GHC's `(4 * 5) \`rem\` 3` = 2 — a silent regrouping of valid
-- Haskell arithmetic.

main :: IO ()
main = do
    -- same level as *: left-associated chain
    print (4 * 5 `rem` 3)          -- (4*5) rem 3 = 2
    print (4 * 5 `quot` 3)         -- (4*5) quot 3 = 6
    print (20 `rem` 6 * 2)         -- (20 rem 6) * 2 = 4
    print (20 `quot` 6 * 2)        -- (20 quot 6) * 2 = 6
    -- same level as div/mod: left-associated with them
    print (100 `quot` 5 `rem` 3)   -- (100 quot 5) rem 3 = 2
    print (100 `div` 5 `quot` 3)   -- (100 div 5) quot 3 = 6
    -- binds tighter than + (6)
    print (1 + 7 `rem` 3)          -- 1 + (7 rem 3) = 2
    print (1 + 7 `quot` 3)         -- 1 + (7 quot 3) = 3
    -- negative operands: quot/rem truncate toward zero (unchanged)
    print ((-7) `quot` 2)          -- -3
    print ((-7) `rem` 2)           -- -1
    -- div/mod grouping pinned alongside (was already correct)
    print (4 * 5 `mod` 3)          -- (4*5) mod 3 = 2
    print (4 * 5 `div` 3)          -- (4*5) div 3 = 6
