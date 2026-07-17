-- Multiplicity polymorphism and the composability of linear (`%1`) values:
-- everything here must COMPILE and behave exactly as the same program with
-- plain arrows would (the annotations erase). The rejection side — a double
-- use that is only reachable through a polymorphic helper, a local
-- function, or a non-IO bind — is covered by the linear_rejects_* tests in
-- run_mll.rs.

data Token = Token Integer

useOnce :: Token %1 -> Integer
useOnce (Token n) = n

count :: Token -> Integer
count (Token n) = n + n

-- A multiplicity-polymorphic helper: `%m` is chosen by each caller. Applied
-- to a `%1` function the argument stays linear; applied to an unrestricted
-- one it is unrestricted. Inside the definition `m` is rigid, so forwarding
-- x to f (an arrow at that same `m`) is the one permitted use.
apply :: (a %m -> b) -> a %m -> b
apply f x = f x

-- Same, with the argument first: the `%m` arrow need not be the last one.
pipeTo :: a %m -> (a %m -> b) -> b
pipeTo x f = f x

-- The linear value stays linear THROUGH the helper (m instantiates to One
-- at this use, because useOnce's arrow is `%1`).
viaApply :: Token %1 -> Integer
viaApply t = apply useOnce t

-- The same helper at an unrestricted instantiation (m = Many).
viaApplyMany :: Token -> Integer
viaApplyMany t = apply count t

-- An alias of a `%m`-typed function keeps the SAME rigid m (the let scheme
-- must not re-quantify a multiplicity that is free in the environment).
viaAlias :: (a %m -> b) -> a %m -> b
viaAlias f x = let g = f in g x

-- A linear value forwarded through a local `where` function: the pass
-- infers that `go` uses its parameter exactly once and charges the call
-- accordingly.
viaWhere :: Token %1 -> Integer
viaWhere t = go t
  where go x = useOnce x

-- The same through a `let`-bound local function (desugars to a lambda).
viaLet :: Token %1 -> Integer
viaLet t = let g x = useOnce x in g t

-- A RECURSIVE local forwarder: one use per evaluation path, established by
-- the fixpoint over the local group.
viaRecursion :: Token %1 -> Integer -> Integer
viaRecursion t n = go t n
  where go x k = if k > 0 then go x (k - 1) else useOnce x

-- A linear value consumed exactly once inside a non-IO do-block. The
-- consumption sits in the bind's ACTION (always evaluated), not in its
-- continuation: Maybe's bind skips the continuation on Nothing, so a
-- consumption there would leak on that path and is rejected (exactly-once,
-- as in GHC — Maybe's bind cannot promise to run a linear continuation).
-- The scalar `n` aliases the consumption and is forced by the continuation.
viaMaybe :: Token %1 -> Maybe Integer
viaMaybe t = do
  n <- Just (useOnce t)
  pure (n + 1)

-- A `%1` consumer defined via the polymorphic helper in IO, mixed into an
-- ordinary do-block. (The scalar where-binding consumes the token exactly
-- once; only the plain Integer is handed to assert.)
shred :: Token %1 -> IO ()
shred t = assert (v == 7) "shred: consumed the right token"
  where v = apply useOnce t

main :: IO ()
main = do
  assert (viaApply (Token 21) == 21) "linear through the %m helper"
  assert (viaApplyMany (Token 4) == 8) "the same helper at %Many"
  assert (pipeTo (Token 5) useOnce == 5) "%m arrow in leading position"
  assert (viaAlias useOnce (Token 6) == 6) "alias keeps the rigid m"
  assert (viaWhere (Token 11) == 11) "linear through a where-function"
  assert (viaLet (Token 12) == 12) "linear through a let-function"
  assert (viaRecursion (Token 13) 3 == 13) "linear through a recursive forwarder"
  assert (viaMaybe (Token 41) == Just 42) "linear consumed once under a Maybe bind"
  putStrLn "before"
  shred (Token 7)
  putStrLn "after"
-- expect: before
-- expect: after
