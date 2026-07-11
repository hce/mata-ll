-- Regression: matching a nested pattern under `Just` must force the payload
-- to WHNF before destructuring it. `Just (mk ...)` builds the payload lazily
-- (a thunk), and matching `Just (a, b)` / `Just (Con ..)` forces each
-- constructor level to WHNF — so codegen must `__force` the Just payload
-- before indexing its sub-fields. Previously the `Just` special case in
-- collect_pattern_conditions built `(_s)[1][1]` without the force that every
-- other constructor/tuple field already applied via field_path, so a thunked
-- payload was indexed as a raw function -> "arithmetic on a function value".

-- payload is a thunk (built from non-trivial expressions)
mkPair :: Integer -> Maybe (Integer, Integer)
mkPair n = Just (slow n, slow (n + 1))
  where slow x = x + 0

usePair :: Maybe (Integer, Integer) -> Integer
usePair m = case m of
  Just (a, b) -> a + b
  Nothing     -> 0

-- nested constructor under Just (Just (Just x))
data Box = Box Integer

mkBox :: Integer -> Maybe Box
mkBox n = Just (Box (n + 0))

useBox :: Maybe Box -> Integer
useBox m = case m of
  Just (Box v) -> v
  Nothing      -> 0

-- Just binding a plain variable must stay lazy (no gratuitous force):
-- the payload is a bottom that is never demanded.
lazyPayload :: Bool -> Integer
lazyPayload b =
  let m = Just (error "boom") :: Maybe Integer
  in case m of
       Just _  -> if b then 1 else 2
       Nothing -> 0

main :: IO ()
main = do
  assert (usePair (mkPair 5) == 11) "Just (a,b): payload forced before tuple destructure"
  assert (useBox (mkBox 7) == 7) "Just (Box v): nested constructor forced"
  assert (lazyPayload False == 2) "Just _ binds payload lazily (bottom not demanded)"
  putStrLn "nested_just_pattern ok"
