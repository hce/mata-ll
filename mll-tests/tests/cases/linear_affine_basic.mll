-- Linear (affine) types: `a %1 -> b` arrows. Everything here must COMPILE
-- and behave exactly as the same program with plain arrows would — the
-- multiplicity is a type-checking discipline that erases after checking.
-- The rejection side (double use, unrestricted flow, aliasing) is covered by
-- the linear_rejects_* tests in run_mll.rs.

data Token = Token Integer

-- A %1 consumer may destructure its argument; the scalar field is plain
-- data and may be used freely (scalar exemption — n carries no resource).
useOnce :: Token %1 -> Integer
useOnce (Token n) = n + n

-- Session style: consume the resource once and hand it back.
step :: Token %1 -> (Token, Integer)
step t = (t, 5)

-- One use per branch is one use (branches are alternatives).
branchy :: Token %1 -> Integer -> Integer
branchy t n = if n > 0 then useOnce t else useOnce t + 1

-- Recursion: each path still uses the argument at most once.
countdown :: Token %1 -> Integer -> Integer
countdown t n = if n > 0 then countdown t (n - 1) else useOnce t

-- A %1 function applied through a higher-order %1 parameter, with the
-- lambda's binder checked against the propagated multiplicity.
withToken :: (Token %1 -> Integer) -> Integer
withToken f = f (Token 21)

-- A scalar where-binding may be used repeatedly: the thunk is memoized, so
-- the %1 argument is consumed exactly once no matter how often `go` is read.
memoized :: Token %1 -> Integer
memoized t = go + go
  where go = useOnce t

-- The explicit unrestricted spellings are accepted and mean a plain arrow.
plainMany :: Token %Many -> Integer
plainMany (Token n) = n + n

plainManyTick :: Token %'Many -> Integer
plainManyTick t = plainMany t + 1

-- A tuple-pattern %1 argument: each component used once.
both :: (Token, Token) %1 -> Integer
both (a, b) = useOnce a + useOnce b

-- IO: a %1 action consumer, used once inside a do-block, with the argument
-- threaded through other statements.
shred :: Token %1 -> IO ()
shred (Token n) = assert (n == 7) "shred: consumed the right token"

main :: IO ()
main = do
  assert (useOnce (Token 21) == 42) "useOnce destructures"
  case step (Token 3) of
    (t2, five) -> do
      assert (five == 5) "step returns the extra"
      assert (useOnce t2 == 6) "step hands the resource back"
  assert (branchy (Token 4) 1 == 8) "branch join, then-side"
  assert (branchy (Token 4) 0 == 9) "branch join, else-side"
  assert (countdown (Token 2) 3 == 4) "recursion uses once per path"
  assert (withToken (\t -> useOnce t) == 42) "lambda through %1 HOF"
  assert (memoized (Token 5) == 20) "memoized scalar where-binding"
  assert (plainManyTick (Token 8) == 17) "%Many and %'Many spellings"
  assert (both (Token 1, Token 2) == 6) "tuple-pattern %1 argument"
  putStrLn "before"
  shred (Token 7)
  putStrLn "after"
-- expect: before
-- expect: after
