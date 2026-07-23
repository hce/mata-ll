-- The tuple constructor as a prefix function: `(,) :: a -> b -> (a, b)`,
-- `(,,) :: a -> b -> c -> (a, b, c)`, etc. Desugars to a multi-param lambda,
-- so it works fully applied, partially applied, and passed higher-order.

fst3 :: (String, Int, Int) -> String
fst3 (a, _, _) = a

main :: IO ()
main = do
  -- Fully applied.
  assert ((,) 1 2 == (1, 2)) "(,) fully applied"
  -- Partial application (the reported use: `map ((,) k) xs`).
  let tagged = map ((,) "k") [1, 2, 3]
  assert (tagged == [("k", 1), ("k", 2), ("k", 3)]) "(,) partial via map"
  -- Passed higher-order to a function that calls it with both args at once.
  assert (zipWith (,) [1, 2] [10, 20] == [(1, 10), (2, 20)]) "(,) via zipWith"
  -- In a composition pipeline: ((,) k) . f
  let f = ((,) "n") . (\x -> x + 1)
  assert (f 4 == ("n", 5)) "(,) in composition"
  -- Triple constructor.
  assert (fst3 ((,,) "hi" 1 2) == "hi") "(,,) triple"
