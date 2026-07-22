-- Regression: first-class `>>=`/`>>` — non-lambda RHS and operator-as-value.
-- Under the calling convention, applying an IO-typed function performs the
-- action and returns its result (with at most one pending pure box), so
-- `m >>= f` must forward `f x` through the runner rather than call its
-- result. Before the fix, `step 1 >>= print` printed and then crashed with
-- "attempt to call a nil value", and a first-class `(>>=)` was emitted
-- verbatim into the Lua output (a syntax error). Both shapes were masked
-- while `>>=` was wrongly right-associative; the infixl 1 fixity fix made
-- them reachable in ordinary code.

step :: Integer -> IO Integer
step n = do
  print n
  return (n + 1)

double :: Integer -> IO Integer
double n = return (n * 2)

-- (>>=) passed as an ordinary function value.
apply2 :: (IO Integer -> (Integer -> IO Integer) -> IO Integer) -> IO Integer
apply2 b = b (step 10) step

-- (>>) passed as an ordinary function value.
thenOp :: (IO Integer -> IO () -> IO ()) -> IO ()
thenOp t = t (step 20) (putStrLn "then done")

main :: IO ()
main = do
  -- non-lambda RHS
  step 1 >>= print
  -- chained non-lambda binds: infixl 1, so (step 1 >>= step) >>= print
  step 1 >>= step >>= print
  -- >> between non-lambda actions
  step 5 >> print 99
  -- non-lambda continuation whose action ends in `return`
  step 30 >>= double >>= print
  -- bind as a first-class value
  r <- apply2 (>>=)
  print r
  thenOp (>>)
  -- first-class bind bound locally
  let b = (>>=)
  b (step 40) print
