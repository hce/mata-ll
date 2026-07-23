-- Linear types: `a %1 -> b` arrows — a `%1` value must be consumed EXACTLY
-- once. Everything here must COMPILE and behave exactly as the same program
-- with plain arrows would — the multiplicity is a type-checking discipline
-- that erases after checking. The rejection side (double use, zero uses,
-- unrestricted flow, aliasing, dropped paths) is covered by the
-- linear_rejects_* tests in run_mll.rs.

data Token = Token Int
data Box = Box Token

-- A %1 consumer may destructure its argument; the scalar field is tracked
-- exactly-once like every other part of the value (GHC parity — no scalar
-- exemption), and the one strict `*` here is its one consumption.
useOnce :: Token %1 -> Int
useOnce (Token n) = n * 2

-- Session style: consume the resource once and hand it back.
step :: Token %1 -> (Token, Int)
step t = (t, 5)

-- One use per branch is one use (branches are alternatives).
branchy :: Token %1 -> Int -> Int
branchy t n = if n > 0 then useOnce t else useOnce t + 1

-- Recursion: each path still consumes the argument exactly once.
countdown :: Token %1 -> Int -> Int
countdown t n = if n > 0 then countdown t (n - 1) else useOnce t

-- A tainted case: the binder aliases the %1 value and is itself consumed
-- exactly once.
unwrap :: Box %1 -> Int
unwrap b = case b of
  Box t -> useOnce t

-- Exactly-once through every alternative of a case (only one branch runs,
-- and each consumes the tainted binder once).
caseBoth :: Box %1 -> Int -> Int
caseBoth b n = case b of
  Box t -> case n > 0 of
    True -> useOnce t
    False -> useOnce t + 1

-- A %1 function applied through a higher-order %1 parameter, with the
-- lambda's binder checked against the propagated multiplicity.
withToken :: (Token %1 -> Int) -> Int
withToken f = f (Token 21)

-- A scalar where-binding built from a %1 value is tracked exactly-once
-- like the value itself: used once here, so the token is consumed exactly
-- once. (Reading `go` twice — the old scalar-memoization relaxation — now
-- rejects; see linear_rejects_scalar_where_binding_double_use in
-- run_mll.rs.)
onceVia :: Token %1 -> Int
onceVia t = go + 1
  where go = useOnce t

-- The explicit unrestricted spellings are accepted and mean a plain arrow.
plainMany :: Token %Many -> Int
plainMany (Token n) = n + n

plainManyTick :: Token %'Many -> Int
plainManyTick t = plainMany t + 1

-- A tuple-pattern %1 argument: each component used once.
both :: (Token, Token) %1 -> Int
both (a, b) = useOnce a + useOnce b

-- IO: a %1 action consumer, used once inside a do-block, with the argument
-- threaded through other statements. The tracked scalar `n` is consumed by
-- the strict `==` in the if-condition — handing it to an unrestricted
-- function (like `assert`) would be rejected, since a plain arrow makes no
-- exactly-once promise.
shred :: Token %1 -> IO ()
shred (Token n) = if n == 7 then putStrLn "shred: consumed the right token" else putStrLn "shred: WRONG TOKEN"

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
  assert (unwrap (Box (Token 9)) == 18) "tainted case binder consumed once"
  assert (caseBoth (Box (Token 4)) 1 == 8) "case join, True side"
  assert (caseBoth (Box (Token 4)) 0 == 9) "case join, False side"
  assert (withToken (\t -> useOnce t) == 42) "lambda through %1 HOF"
  assert (onceVia (Token 5) == 11) "scalar where-binding consumed once"
  assert (plainManyTick (Token 8) == 17) "%Many and %'Many spellings"
  assert (both (Token 1, Token 2) == 6) "tuple-pattern %1 argument"
  putStrLn "before"
  shred (Token 7)
  putStrLn "after"
-- expect: before
-- expect: shred: consumed the right token
-- expect: after
