-- The other direction of the binder rule: a binder whose own source proves
-- a value — a parameter every call site delivers concrete, a strict let or
-- where value, a bound action yielding WHNF — is read bare even when it is
-- named like a suspended enclosing binding (a thunked top-level CAF, a lazy
-- parameter). The emitted Lua is byte-identical to the unshadowed twin
-- (codegen_shape.rs). 2026-10 fresh-eyes review (L5).
module Main where

lazyTop :: Int
lazyTop = length [(), ()] + 40

strictParam :: Int -> Int
strictParam lazyTop = lazyTop * 2 + 1

strictLet :: Int -> Int
strictLet n = let lazyTop = n * 3 in lazyTop + 1

strictWhere :: Int -> Int
strictWhere n = lazyTop + 2
  where
    lazyTop = n * 4

boundAction :: Int -> IO ()
boundAction n = do
    lazyTop <- pure (n + 1)
    print (lazyTop * 10)

lazyParam :: Int -> Int -> Int
lazyParam lazyTop n = if n > 0 then n else lazyTop

innerOverLazyParam :: Int -> Int
innerOverLazyParam lazyTop = case map (+ 1) [lazyTop] of
    (v : _) -> let lazyTop = v * 2 in lazyTop + 3
    [] -> 0

main :: IO ()
main = do
    print lazyTop
    print (strictParam 5)
    print (strictLet 5)
    print (strictWhere 5)
    boundAction 5
    print (lazyParam (length [(), ()] + 7) 0)
    print (innerOverLazyParam (length [()] + 1))
