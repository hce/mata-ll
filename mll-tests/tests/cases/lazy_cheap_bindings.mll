-- Regression: cheap-eagerness soundness (Faxén-style eager let bindings).
--
-- A let/where binding may only be evaluated eagerly (assigned strictly)
-- when doing so cannot force a suspended computation. Previously is_cheap
-- treated every Var as cheap, so a binding like `z = y + 1` was evaluated
-- eagerly and forced `y = error "boom"` even though z was never demanded.
-- GHC never touches y in these programs; neither may we. A Var only counts
-- as cheap-to-force when its referent is provably WHNF (pattern-bound,
-- demand-strict parameter, or a prior binding assigned strictly under the
-- same rule).

-- let-binding: arithmetic over a bottom thunk, never demanded
letBottom :: Bool -> Int
letBottom x = let y = error "boom"
                  z = y + 1
              in if x then z else 0

-- transitive: the bottom flows through a second unused cheap-looking binding
letBottomChain :: Bool -> Int
letBottomChain x = let y = error "boom"
                       z = y + 1
                       w = z + 1
                   in if x then w else 0

-- where-clause form (separate emission path from let)
whereBottom :: Bool -> Int
whereBottom x = if x then z else 0
  where
    y = error "boom"
    z = y + 1

-- a diverging (not erroring) binding guarded behind a branch;
-- non-tail recursion so an eager force fails fast instead of hanging
diverge :: Int -> Int
diverge n = n + diverge (n + 1)

letDiverge :: Bool -> Int
letDiverge x = let d = diverge 0
                   z = d + 1
               in if x then z else 0

-- if-binding fast path: the condition is a bottom, the binding unused
ifBottom :: Bool -> Int
ifBottom x = let z = if error "boom" then 1 else (2 :: Int)
             in if x then z else 0

-- constructor application over a bottom thunk: building `Just y` must not
-- force y (the constructed value is WHNF, its field stays suspended)
isJustI :: Maybe Int -> Int
isJustI (Just _) = 1
isJustI Nothing = 2

conBottom :: Bool -> Int
conBottom x = let y = error "boom"
                  z = Just y
              in if x then isJustI z else 0

-- do-block let bindings (flattened bind-chain emission path)
doLetBottom :: Bool -> IO ()
doLetBottom b = do
    let y = error "boom"
    let z = y + 1
    if b then putStrLn (show z) else pure ()

-- guard against over-tightening: bindings over provably-WHNF variables
-- must still evaluate correctly (and eagerly — see the emitted-Lua check
-- in run_mll.rs). n is literal-bound, x is a demand-strict parameter.
whnfCheap :: Int -> Int
whnfCheap x = let n = 5
                  m = n + x
              in m + n

-- demand-strict parameter feeding a chain of cheap bindings
strictParamChain :: Int -> Int
strictParamChain n = let a = n + 1
                         b = a * 2
                     in a + b

-- ============================================================
-- demand.rs over-claim fixes (they fed the binding eagerization,
-- so an over-claim there would reintroduce unsound eagerness)
-- ============================================================

bottomFn :: Int -> Int
bottomFn _ = error "demand-boom"

-- A later guard's condition must not make a parameter strict: it only
-- runs when the earlier guards failed. Previously all guard conditions
-- were unioned, so y was forced at entry even when the first guard
-- matched (GHC never touches y here).
guardLazy :: Int -> Int -> String
guardLazy x y
  | x > 0 = "pos"
  | y > 0 = "ypos"
  | otherwise = "neither"

-- && / || short-circuit: the right operand must not be demanded.
-- (Multi-clause so the function is not inlined away.)
andLazy :: Bool -> Bool -> Bool
andLazy True b = b
andLazy False b = False && b

-- ++ is lazy in its right operand (the emitted code thunks it).
appendLazy :: [Int] -> [Int] -> [Int]
appendLazy xs ys = xs ++ ys

main :: IO ()
main = do
    assert (letBottom False == 0) "let: bottom binding not demanded"
    assert (letBottomChain False == 0) "let: bottom through unused chain"
    assert (whereBottom False == 0) "where: bottom binding not demanded"
    assert (letDiverge False == 0) "let: diverging binding not demanded"
    assert (ifBottom False == 0) "let: if-binding with bottom condition"
    assert (conBottom False == 0) "let: constructor over bottom field"
    doLetBottom False
    assert (whnfCheap 10 == 20) "whnf bindings still compute correctly"
    assert (strictParamChain 4 == 15) "strict-param chain still correct"
    assert (guardLazy 1 (bottomFn 0) == "pos") "later guard condition stays lazy"
    assert (andLazy False (0 < bottomFn 0) == False) "&& right operand stays lazy"
    assert (head (appendLazy [1] (error "append-boom")) == 1) "++ right operand stays lazy"
    putStrLn "lazy_cheap_bindings ok"
