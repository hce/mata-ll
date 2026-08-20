-- Demand analysis applies cross-function strictness rows and special-name
-- rules (`otherwise`, backtick operators) BY NAME. A local binder —
-- parameter, pattern variable, let/where binding — shadows the global
-- meaning, so those name-keyed rules must be suppressed for it.
-- Regression: `apply inc x = inc x` looked up the row of the TOP-LEVEL
-- `inc` (strict), marked `apply` strict in x, and the emitted entry-force
-- evaluated `error "boom"` where GHC returns the lambda's result without
-- touching x. Suppression only under-claims (keeps values lazy), so every
-- fix here is safe by construction.

-- strict top-level functions whose names the locals below shadow
inc :: Int -> Int
inc n = n + 1

w :: Int -> Int
w n = n * 2

-- parameter shadowing a strict global: x must stay lazy
apply :: (Int -> Int) -> Int -> Int
apply inc x = inc x

-- where-value shadowing a strict global
useW :: Int -> Int
useW x = w x
  where w = \_ -> 5

-- `otherwise` as a (False) parameter: the then-branch is NOT
-- unconditional, so its demands must not be claimed at entry
pickD :: Bool -> Int -> Int
pickD otherwise y | otherwise = y + 1
                  | True      = 1

-- backtick operator shadowed by a parameter lazy in its second operand
applyDiv :: (Int -> Int -> Int) -> Int -> Int
applyDiv div x = 10 `div` x

main :: IO ()
main = do
    print (apply (\_ -> 42) (error "boom"))
    print (useW (error "boom"))
    print (pickD False (error "boom"))
    print (applyDiv (\a _ -> a) (error "boom"))
    -- unshadowed controls: the real rows still apply
    print (inc 4)
    print (w 4)
    print (pickD True 9)
