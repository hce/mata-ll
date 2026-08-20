-- A local binder (parameter, pattern variable, let/where binding) shadows
-- every special-name and fast-path meaning of its name — record
-- accessors, inline candidates, and the specially-lowered callees
-- (seq/div/quot/rem/pure/return/try/otherwise).
-- Regression: the codegen fast-paths matched the callee by NAME without
-- consulting local_vars, so `apply width = width 5` emitted record
-- indexing on 5, a parameter named `double` inlined the top-level
-- double's body, and a parameter named `div`/`seq`/`pure`/`try` was
-- replaced by the runtime primitive.

data R = R { width :: Int }

-- record accessor name as a parameter (applied as a function)
applyWidth :: (Int -> Int) -> Int
applyWidth width = width 5

-- top-level inline candidate shadowed by a parameter
double :: Int -> Int
double x = x + x

applyDouble :: (Int -> Int) -> Int
applyDouble double = double 5

-- specially-lowered callee names as parameters
applyDiv :: (Int -> Int -> Int) -> Int
applyDiv div = div 10 2

applySeq :: (Int -> Int -> Int) -> Int
applySeq seq = seq 1 2

applyPure :: (Int -> Int) -> Int
applyPure pure = pure 41

applyTry :: (Int -> Int) -> Int
applyTry try = try 20

-- `otherwise` as a parameter: in a guard it is the (False) parameter,
-- not the Prelude constant, so the second guard is taken
pick :: Bool -> Int
pick otherwise | otherwise = 1
               | True      = 2

main :: IO ()
main = do
    print (applyWidth (+ 1))          -- 6   (accessor would index 5)
    print (width (R 9))               -- 9   (real accessor still works)
    print (applyDouble (* 3))         -- 15  (inlined body would give 10)
    print (double 5)                  -- 10  (real function still inlines)
    print (applyDiv (+))              -- 12  (__mll_div_fn would give 5)
    print (applySeq (\a _ -> a))      -- 1   (__mll_seq would give 2)
    print (applyPure (+ 1))           -- 42
    print (applyTry (* 2))            -- 40
    print (pick False)                -- 2   (Prelude otherwise would give 1)
    print (pick True)                 -- 1
