-- A ⊥ CAF — a trapping op the constant folds decline, like a literal
-- zero divisor — is a THUNK: it raises only if demanded, never at
-- module load (GHC evaluates a CAF on first demand; one that is never
-- demanded never runs). Regression: codegen's load-time cheap-value
-- rule used the duplication notion of cheapness (is_cheap, which
-- admits div/mod), so `bad` below ran __mll_div(1, 0) while the
-- module was still loading and the whole program raised.

bad :: Int
bad = 1 `div` 0

badMod :: Int
badMod = 1 `mod` 0

pick :: Int -> Int
pick x = if x > 0 then x else bad + badMod

main :: IO ()
main = print (pick 3)
