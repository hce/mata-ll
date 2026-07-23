-- Regression: a `<-`-bound result of a USER-DEFINED action must not be treated
-- as already-forced (concrete). A user action whose body ends in
-- `pure <non-trivial expr>` compiles to an action closure whose result
-- __mll_run returns UNFORCED (the non-strict `return` contract), so the bound
-- variable can hold a thunk. Before the fix, action_result_is_whnf defaulted
-- to `true` for any non-`return` action, the binder marked the variable
-- concrete, and a strict use compiled to a force-free read:
--     v <- stHelper arr n
--     return (v + 1)      -- emitted bare `v + 1`
-- crashing at runtime with "attempt to perform arithmetic on a table value".
--
-- Also covers the ST->pure boundary: runST must force the state thread's
-- result to WHNF (GHC: demanding `runST m` demands the returned value), so a
-- suspended terminal `pure e` cannot escape as a raw thunk into show/print.

g :: Int -> Int
g x = x + 1

-- Closure-form ST action ending in `pure <application>`: the pure argument is
-- not provably total, so it is suspended, and the run result is a thunk.
stHelper :: STArray s -> Int -> ST s Int
stHelper arr n
  | n > 100   = return 0
  | otherwise = do
        writeSTArray arr 0 n
        x <- readSTArray arr 0
        pure (g x * 2)

-- The bound `v` is used in a strict position (arithmetic): it MUST be forced.
useST :: Int -> Int
useST n = runST (do
    arr <- newSTArrayFromList [n]
    v <- stHelper arr n
    return (v + 1))

-- Same shape through IO: bound result of an applied user action, used strictly.
mkIO :: Int -> IO Int
mkIO n = do
    _ <- return ()
    pure (g n * 2)

-- The runST result itself must be WHNF: `show` reads it force-free.
main :: IO ()
main = do
    assert (useST 5 == 13) "ST: <-bound pure-thunk result forced at strict use"
    assert (show (useST 5) == "13") "runST result is WHNF (show reads it force-free)"
    w <- mkIO 5
    assert (w + 1 == 13) "IO: <-bound pure-thunk result forced at strict use"
    putStrLn "action_result_whnf: all assertions passed"
